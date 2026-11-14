# P0-11 LSP・MCP・ACPの最小クライアント

[`docs/research/P0-11-protocol-clients.md`](../../docs/research/P0-11-protocol-clients.md) の確認に使ったプログラム。製品コードではない。

```sh
nix develop -c cargo build --manifest-path spikes/p0-11-protocol-clients/Cargo.toml
B=spikes/p0-11-protocol-clients/target/debug
$B/mcp-client          # stdio（mcp-server を子プロセスで起動）と Streamable HTTP（ループバック）
$B/acp-client          # acp-agent を子プロセスで起動
# LSP（marksman はフレークの nixpkgs から取得）
M=$(nix build --inputs-from . nixpkgs#marksman --print-out-paths --no-link)
$B/lsp-client $M/bin/marksman server
```
