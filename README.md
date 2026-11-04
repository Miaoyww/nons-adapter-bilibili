# Nons Bilibili 适配器

来源 `bilibili`，以 Rust 实现并编译为 WASM Component。网络、WBI 密钥缓存和诊断经通用 Host 访问。

提供二维码登录、视频搜索、音轨播放解析和 MV 搜索/解析，尚未接入音乐库。Cookie 作为不透明凭据交给 AccountManager 统一保存，登录后搜索和 playurl 自动携带；资料校验失败会撤销当前本地会话。Bilibili 视频作为曲目来源使用稳定的 BV 号；MV 是独立 `mv` 引用，不改写歌曲身份。声明 `lyricMatch` 后从 `/x/player/wbi/v2` 的 `bgm_info.music_title` 提取歌曲名，供宿主歌词服务匹配；缺少 BGM 信息时由宿主回退视频标题。

协议层对照 `BBPlayer` 的 Web API 实现：固定 `api.bilibili.com`，统一 Cookie、移动端 UA、`Referer`、`Origin`，解 `{code,message,data}` 信封；WBI 使用同序参数过滤、编码、MD5、`wts` 和 `w_rid`，密钥从 `/x/web-interface/nav` 提取并按日保存在适配器状态。playurl 使用 `fnval=4048`、`qlt`、`voice_balance=1` 和桌面浏览器 UA，保留同音质 `backup_url`。音频优先级为 Dolby、Hi-Res、指定音质、最高音质、旧视频 `durl`；MV 在 DASH 视频中优先 AVC 1080p，再退到最高可用编码。

## 构建

```powershell
$env:NONS_ROOT = 'C:\Projects\NonsPlayersProject\Nons'
node scripts/build.mjs
```

构建生成 `dist/adapter.component.wasm`，并复制到 Nons 的随包适配器目录。

## 测试

```powershell
cargo test
node scripts/build.mjs
node --test tests/component.test.mjs
```

Rust 单测覆盖 WBI、BV 号、页面游标、音轨与 MV 选择；Component harness 经受控 Host 执行真实 WASM。

宿主公网二维码引导测试：

```powershell
cargo test --manifest-path ..\..\..\Nons\app\src-tauri\Cargo.toml --lib bilibili_component_live_qr_bootstrap -- --ignored --nocapture
```

默认只生成二维码并取消挑战；设置 `NONS_BILIBILI_QR_WAIT=1` 后最多等待 180 秒供人工扫码。

## 已知差异

- Bilibili 的 WBI `sub_key` 按 BBPlayer 移动端实现保留 URL 扩展名；Android Kotlin 实现会去掉扩展名，两者并不完全一致。
- WBI 密钥“当天缓存”在 guest 内按 UTC 日界计算，BBPlayer 使用设备本地日界。
- BBPlayer 的 CDN 测速、偏好和原生断流 Range 回退未移植；资源会返回主地址与备用列表，当前宿主只由 MV 代理消费备用列表。
- BBPlayer 只实现音频；MV 的 DASH `video` 选择、AVC 优先规则和本地代理传输是本适配器新增部分。
- 短信/Geetest 与手动 Cookie 登录尚未接入，当前使用二维码登录；Hi-Res/Dolby 是否可用取决于账号和视频本身。

## 许可

协议行为依据同工作区的 `BBPlayer` 实现；本项目代码使用 [MIT](License)。
