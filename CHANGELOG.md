# Changelog

## Unreleased

### Changed

- 契约版本升至 `2`，移除 `descriptor.coverHosts`、`permissions.httpHosts` 和 `permissions.websocketHosts`。
- Host transport 升至 `nons:host/transport@1.1.0`；平台请求 `timeout-ms` 改为 `0`，由 Host 使用默认行为。
- HTTP 请求固定跟随重定向，并映射 Host 返回的最终响应 URL。
- 移除调用参数和回复的 64KiB 限制，保留平台分页、签名和音质限制。
- 构建时幂等规范化主仓旧 `host.wit` 的包版本、redirect 和响应 URL 字段。
- 包版本升至 `1.0.0`。
