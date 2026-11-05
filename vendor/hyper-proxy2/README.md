# hyper-proxy2 (Vinilo patch)

Upstream [hyper-proxy2](https://github.com/siketyan/hyper-proxy2) 0.1.0, with
TLS deps raised to `hyper-rustls` 0.27 / `tokio-rustls` 0.26 so the crate no
longer pulls the vulnerable rustls 0.22 / rustls-webpki 0.102 line.

Do not publish; used only via `[patch.crates-io]` in the workspace root.
