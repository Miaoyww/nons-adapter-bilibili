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
const LONG_TITLE = '晴'.repeat(70_000)
const FINAL_URL = 'https://user:secret@127.0.0.1:8443/final?token=redirected'

async function component() {
  const requests = []
  const timeouts = []
  const redirects = []
  const states = new Map()
  let credential = new Uint8Array()
  let published
  let pollCount = 0
  const transport = {
    httpRequest(request) {
      requests.push(request)
      timeouts.push(request.timeoutMs)
      redirects.push(request.followRedirects)
      const url = new URL(request.url)
      let body
      const headers = [{ name: 'content-type', value: 'application/json' }]
      if (url.pathname === '/start')
        return {
          status: 200,
          headers,
          url: FINAL_URL,
          body: new TextEncoder().encode('{}')
        }
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
                title: `<em class="keyword">晴天</em>${LONG_TITLE}`,
                pic: 'https://user:secret@192.168.1.10:8443/archive/cover.jpg?token=public',
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
                  base_url: 'https://user:secret@192.168.1.10:8443/audio.m4s?token=audio',
                  backup_url: [
                    'https://backup:secret@192.168.1.11:9443/audio.m4s?token=backup'
                  ]
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
      else if (url.pathname === '/x/player/wbi/v2')
        body = {
          code: 0,
          data: { bgm_info: { music_id: 7, music_title: '《晴天》官方 MV' } }
        }
      else if (url.pathname === '/x/passport-login/web/qrcode/generate')
        body = {
          code: 0,
          data: {
            url: 'https://www.bilibili.com/h5/mobile-appeal',
            qrcode_key: 'qr-key'
          }
        }
      else if (url.pathname === '/x/passport-login/web/qrcode/poll') {
        if (pollCount++ === 0) body = { code: 0, data: { code: 86090 } }
        else {
          body = { code: 0, data: { code: 0 } }
          headers.push(
            { name: 'set-cookie', value: 'SESSDATA=sess; Path=/; HttpOnly' },
            { name: 'set-cookie', value: 'bili_jct=csrf; Path=/; HttpOnly' },
            { name: 'set-cookie', value: 'DedeUserID=42; Path=/; HttpOnly' }
          )
        }
      } else if (url.pathname === '/x/space/myinfo')
        body = {
          code: 0,
          data: { mid: 42, name: 'Listener', face: '//i0.hdslb.com/avatar.jpg' }
        }
      else body = { code: 0, data: [] }
      return {
        status: 200,
        headers,
        url: request.url,
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
      return credential
    },
    credentialPublish(metadata, data) {
      credential = data
      published = { metadata: JSON.parse(metadata), credential: new TextDecoder().decode(data) }
    },
    credentialReplace(data) {
      credential = data
    },
    credentialInvalidate() {}
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
    timeouts,
    redirects,
    published: () => published,
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
    assert.ok(tracker.data.data.items[0].title.startsWith('晴天'))
    assert.ok(tracker.data.data.items[0].title.length > 64 * 1024)
    assert.equal(tracker.data.data.items[0].artist, 'UP')
    assert.deepEqual(tracker.data.data.items[0].artists, [
      { name: 'UP', reference: { source: 'bilibili', kind: 'artist', id: '7' } }
    ])
    assert.equal(
      tracker.data.data.items[0].cover.original,
      'https://user:secret@192.168.1.10:8443/archive/cover.jpg?token=public'
    )

    const audio = fixture.call('music.resolve-playback', {
      track: tracker.data.data.items[0].reference,
      preferredQuality: 'exhigh',
      allowDowngrade: true
    })
    assert.equal(audio.status, 'ok', JSON.stringify(audio))
    assert.equal(
      audio.data.url,
      'https://user:secret@192.168.1.10:8443/audio.m4s?token=audio'
    )
    assert.deepEqual(audio.data.backupUrls, [
      'https://backup:secret@192.168.1.11:9443/audio.m4s?token=backup'
    ])

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
    for (const request of fixture.requests) {
      assert.equal(request.method, 'GET')
      assert.equal(request.body.length, 0)
      assert.equal(request.followRedirects, true)
    }
    assert.equal(fixture.timeouts.length, fixture.requests.length)
    assert.ok(fixture.timeouts.every((timeout) => timeout === 0))
  } finally {
    fixture.instance.shutdown()
  }
})

test('Component forwards an explicit timeout and the resolved response URL', async () => {
  const fixture = await component()
  try {
    const url = 'https://user:secret@127.0.0.1:8080/start?token=origin'
    const count = fixture.requests.length
    const result = fixture.call('runtime:test-http', { url, timeoutMs: 60_000 })
    assert.equal(result.status, 'ok', JSON.stringify(result))
    assert.equal(result.data.url, FINAL_URL)
    assert.equal(fixture.requests.length, count + 1)
    assert.equal(fixture.timeouts.at(-1), 60_000)
    assert.equal(fixture.redirects.at(-1), true)
    assert.equal(fixture.requests.at(-1).url, url)
  } finally {
    fixture.instance.shutdown()
  }
})

test('Component accepts transport arguments larger than the old 64KiB limit', async () => {
  const fixture = await component()
  try {
    const args = JSON.stringify({
      session: { source: 'bilibili' },
      request: {
        operation: 'search',
        kind: 'track',
        keyword: '晴'.repeat(70_000),
        page: { limit: 20, cursor: null }
      }
    })
    assert.ok(args.length > 64 * 1024)
    const result = JSON.parse(fixture.instance.call('music.business', args))
    assert.equal(result.status, 'ok', JSON.stringify(result).slice(0, 200))
  } finally {
    fixture.instance.shutdown()
  }
})

test('Component QR login publishes the profile with the opaque cookie through Host', async () => {
  const fixture = await component()
  try {
    const tick = JSON.parse(
      fixture.instance.call('runtime:event', JSON.stringify({ kind: 'tick' }))
    )
    assert.deepEqual(tick, { status: 'ok', data: null })
    const challenge = fixture.call('music.account', {
      operation: 'beginLogin',
      method: 'qr'
    })
    assert.equal(challenge.status, 'ok', JSON.stringify(challenge))
    assert.equal(challenge.data.type, 'challenge')
    assert.equal(challenge.data.data.key, 'qr-key')
    const waiting = fixture.call('music.account', {
      operation: 'pollLogin',
      key: 'qr-key'
    })
    assert.equal(waiting.data.data.code, 802)
    const success = fixture.call('music.account', {
      operation: 'pollLogin',
      key: 'qr-key'
    })
    assert.equal(success.data.data.code, 803)
    const result = fixture.published()
    assert.equal(result.metadata.reference.id, '42')
    assert.match(result.credential, /SESSDATA=sess/)
    assert.match(result.credential, /bili_jct=csrf/)
    const profile = fixture.call('music.account', { operation: 'profile' })
    assert.equal(profile.status, 'ok', JSON.stringify(profile))
    assert.equal(profile.data.data.displayName, 'Listener')
  } finally {
    fixture.instance.shutdown()
  }
})

test('Component lyric match extracts the BGM song title', async () => {
  const fixture = await component()
  try {
    const result = fixture.call('music.lyric-match', {
      track: { reference: { source: 'bilibili', kind: 'track', id: 'BV1xx411c7mD:42' } }
    })
    assert.equal(result.status, 'ok', JSON.stringify(result))
    assert.equal(result.data.title, '晴天')
    assert.match(
      fixture.requests.find((request) => request.url.includes('/x/player/wbi/v2')).url,
      /w_rid=/
    )
  } finally {
    fixture.instance.shutdown()
  }
})
