import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile, mkdir } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const project = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const root = process.env.NONS_ROOT
  ? resolve(process.env.NONS_ROOT)
  : resolve(project, '../../../Nons')
const tools = resolve(root, 'packages/wasm-crypto/node_modules/@bytecodealliance')
const output = resolve(root, '.local/bilibili-component-tests')
await mkdir(output, { recursive: true })
const result = spawnSync(
  process.execPath,
  [
    resolve(tools, 'jco/dist/jco.js'),
    'transpile',
    resolve(project, 'dist/adapter.component.wasm'),
    '-o',
    output,
    '--instantiation',
    'async'
  ],
  { encoding: 'utf8' }
)
assert.equal(result.status, 0, result.stderr)
const { instantiate } = await import(pathToFileURL(resolve(output, 'adapter.component.js')).href)
const { WASIShim } = await import(
  pathToFileURL(resolve(tools, 'preview2-shim/dist/common/instantiation.js')).href
)

async function component() {
  const requests = []
  const states = new Map()
  const transport = {
    httpRequest(request) {
      requests.push(request)
      const url = new URL(request.url)
      let body
      if (url.pathname === '/x/web-interface/nav')
        body = {
          code: -101,
          message: '未登录',
          data: {
            wbi_img: {
              img_url: 'https://i0.hdslb.com/bfs/wbi/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png',
              sub_url: 'https://i0.hdslb.com/bfs/wbi/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png'
            }
          }
        }
      else if (url.pathname.endsWith('/search/type'))
        body = {
          code: 0,
          message: '0',
          data: {
            numPages: 2,
            result: [
              {
                type: 'video',
                bvid: 'BV1xx411c7mD',
                title: '<em class="keyword">晴天</em>',
                pic: '//i0.hdslb.com/bfs/archive/cover.jpg',
                author: 'UP',
                duration: '3:15',
                mid: 7
              },
              { type: 'ketang', bvid: '', title: '课堂' }
            ]
          }
        }
      else if (url.pathname === '/x/web-interface/view')
        body = {
          code: 0,
          data: {
            bvid: 'BV1xx411c7mD',
            title: '晴天',
            pic: '//i0.hdslb.com/bfs/archive/cover.jpg',
            duration: 195,
            cid: 42,
            owner: { mid: 7, name: 'UP' },
            pages: [{ cid: 42, part: '晴天', duration: 195 }]
          }
        }
      else if (url.pathname.endsWith('/playurl'))
        body = {
          code: 0,
          data: {
            dash: {
              audio: [
                {
                  id: 30280,
                  base_url: 'https://upos.example/audio.m4s',
                  backup_url: ['https://backup.example/audio.m4s']
                }
              ],
              video: [
                {
                  id: 80,
                  codecid: 7,
                  base_url: 'https://upos.example/video.m4s',
                  backup_url: ['https://backup.example/video.m4s']
                }
              ]
            }
          }
        }
      else body = { code: 0, data: [] }
      return {
        status: 200,
        headers: [{ name: 'content-type', value: 'application/json' }],
        body: new TextEncoder().encode(JSON.stringify(body))
      }
    },
    stateGet(key) {
      return states.get(key)
    },
    stateSet(key, value) {
      states.set(key, value)
    },
    credentialRead() {
      return new Uint8Array()
    }
  }
  const instance = await instantiate(
    async (path) => WebAssembly.compile(await readFile(resolve(output, path))),
    {
      ...new WASIShim().getImportObject(),
      'nons:host/transport': transport,
      'nons:plugin/host': { call: () => 'null' }
    }
  )
  instance.initialize()
  return {
    instance,
    requests,
    call(method, request) {
      return JSON.parse(
        instance.call(method, JSON.stringify({ session: { source: 'bilibili' }, request }))
      )
    }
  }
}

test('Component search, playback and MV keep signed params and mirror URLs', async () => {
  const fixture = await component()
  try {
    const tracker = fixture.call('music.business', {
      operation: 'search',
      kind: 'track',
      keyword: '晴天',
      page: { limit: 20, cursor: null }
    })
    assert.equal(tracker.status, 'ok', JSON.stringify(tracker))
    assert.equal(tracker.data.data.items.length, 1)
    assert.equal(tracker.data.data.items[0].reference.id, 'BV1xx411c7mD')
    assert.equal(tracker.data.data.items[0].title, '晴天')

    const audio = fixture.call('music.resolve-playback', {
      track: tracker.data.data.items[0].reference,
      preferredQuality: 'exhigh',
      allowDowngrade: true
    })
    assert.equal(audio.status, 'ok', JSON.stringify(audio))
    assert.deepEqual(audio.data.backupUrls, ['https://backup.example/audio.m4s'])

    const mvs = fixture.call('music.search-mvs', {
      track: { title: '晴天', artists: ['周杰伦'], durationMs: 195000 },
      query: '晴天 周杰伦',
      page: { limit: 20, cursor: null }
    })
    assert.equal(mvs.status, 'ok', JSON.stringify(mvs))
    const video = fixture.call('music.resolve-mv', { mv: mvs.data.items[0].reference })
    assert.equal(video.status, 'ok', JSON.stringify(video))
    assert.equal(video.data.metadata.actualQuality, '1080p')

    const playurl = fixture.requests.find((request) => request.url.includes('/playurl'))
    assert.match(playurl.url, /fnval=4048/)
    assert.match(playurl.url, /w_rid=/)
    assert.equal(
      playurl.headers.find((header) => header.name === 'User-Agent').value.includes('Edg/'),
      true
    )
  } finally {
    fixture.instance.shutdown()
  }
})
