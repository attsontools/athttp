# AT HTTP

HTTP 请求工具的独立应用与 AT Tool 插件。当前仓库处于 P0：先固定 Native/Data Service 的跨平台协议、单实例 endpoint 和握手契约，业务 UI 与现有 HTTP 能力在后续阶段迁移。

应用身份：`com.attson.http`  
展示名称：HTTP 请求  
制品前缀：`AT.HTTP`

P0 服务协议使用临时 loopback TCP 端口和权限受限 endpoint 文件，不监听固定端口或公网地址。客户端必须先完成带 token 的 `service.handshake`，通过 Host API、Data API 和 Service API 版本协商后才能调用业务方法。

```bash
cargo test
```
