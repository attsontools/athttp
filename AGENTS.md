# AGENTS.md

AT HTTP（Tauri 2 + Vue 3 + Native/Data Service）。独立应用和 AT Tool 插件复用同一 UI、请求核心和 service 协议；平台插件通过 Host API 访问本地 service，UI 不直接访问网络或 SQLite。

## 固定身份

- `appId`: `com.attson.http`
- 展示名称：HTTP 请求
- Release 资产前缀：`AT.HTTP`
- Host API：`2.x`
- Service API：`1.x`
- Data API：`1.x`

## 服务约束

- Native/Data Service 是 `http.sqlite3` 的唯一数据所有者；客户端不得直接并发打开数据库。
- service 只绑定临时 loopback TCP 端口，通过权限受限 endpoint 文件发现端口和 UUID token；不得监听固定端口、局域网地址或公网地址。
- 业务请求前必须完成 `service.handshake`；版本不兼容返回稳定错误 `HTTP_SERVICE_VERSION_UNSUPPORTED`。
- service 二进制与 UI 同版本发布，并在同一插件 manifest、Publisher 签名和文件 hash 中校验。
- SSE / WebSocket 消息保持内存 buffer（2000 条、5 MB）和 generation 语义，不做自动重连或消息持久化。

## 开发约定

- 纯协议、版本协商、endpoint 和 service 状态机先写测试再实现；不引入 jsdom。
- 不引入新依赖，除非跨平台 service 能力确实无法由已有 Rust/Tokio 能力实现。
- 不写 emoji；提交使用 conventional commit。
- Publisher 私钥不得写入仓库、日志、测试夹具或构建产物。
