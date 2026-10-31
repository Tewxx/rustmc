# Playing rustmc from Prism Launcher

rustmc runs as Prism's wrapper command: Prism still downloads 1.8.9 and signs you in, then hands rustmc the
launch data (account, assets, client jar, window size, quick-join server) instead of starting Java.

1. Build: `cargo build --release -p mc-client`.
2. Run `tools/prism/make-instance.sh` (or pass the path to your rustmc binary). It writes `rustmc-prism-instance.zip`.
3. In Prism: Add Instance > Import, pick that zip. Make sure you are signed in to a Microsoft account.
4. Launch the "rustmc" instance (or right-click > Launch, or `prismlauncher --launch <id> --server host:port`).
5. Optional: set `RUSTMC_LAUNCH_LOG=<file>` to record what Prism sent (access token redacted).

Notes: options.txt, server-packs.json, screenshots and crash logs live in the instance's `minecraft/` folder.
Without Prism, rustmc looks for the jar/assets in `%APPDATA%\.minecraft`, `~/Library/Application Support/minecraft`
or `~/.minecraft`. The built-in `--online` device-code login remains as a fallback.
