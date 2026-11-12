# AT HTTP

HTTP 请求工具的独立应用与 AT Tool 插件。独立版和插件版复用同一套 Vue UI、HTTP / SSE / WebSocket 核心与 Native/Data Service；插件 UI 通过 AT Tool Host API 2.x 访问 service，不直接访问网络、SQLite 或任意本地路径。

应用身份：`com.attson.http`  
展示名称：HTTP 请求  
制品前缀：`AT.HTTP`

service 使用临时 loopback TCP 端口和权限受限 endpoint 文件，不监听固定端口或公网地址。客户端必须先完成带 token 的 `service.handshake`，通过 Host API、Data API 和 Service API 版本协商后才能调用业务方法。service 是新客户端的单一数据所有者，首次接管已有 `http.sqlite3` 时会备份、迁移并执行完整性检查。

```bash
corepack pnpm test
corepack pnpm build
corepack pnpm build:plugin
cargo test
```

本地插件包使用 `corepack pnpm package:plugin` 生成到 `artifacts/`。正式 Publisher、Catalog 和多平台 Release 在后续发行阶段接入。
