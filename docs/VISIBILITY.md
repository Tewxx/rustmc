# Section visibility

Each single-section mesh records the six-face connectivity computed by vanilla's `VisGraph`: opaque blocks are impassable, connected open cells link every boundary face they reach, fewer than 256 opaque blocks are treated as fully visible, and a full opaque section has no connections. The bitset is compared against deterministic traces generated from the MCP 1.8.9 implementation:

```sh
tools/extract/extract.sh visgraph
cp golden/visgraph.json crates/mc-render/tests/data/visgraph.json
cargo test -p mc-render visibility::tests
```

Traversal is deliberately conservative: the camera section and absent or dirty graphs are open, each possible entry face is tracked, models extending outside their own section are never culled, and sections outside the bounded search are drawn. A changed graph invalidates the cached traversal immediately. While graphs are changing, or an incremental traversal has not finished, all sections are drawn. Traversal is limited to 2,048 nodes per frame and disabled for search radii above 32 sections. Set `RUSTMC_NO_CAVE_CULL=1` to disable the optimization for comparisons.

These graph and traversal tests do not prove same-world rendered pixel parity or an FPS improvement. Validate the renderer with and without cave culling on supported GPUs before making either claim.

A synthetic headless release CPU benchmark (`cargo test --release -p mc-render visibility_headless_bench -- --ignored --nocapture`) placed a complete section wall through the search region. On this shared Mac, two runs at radius 18 reached 11,840 of 21,904 sections in six steps, taking 2.39–5.00 ms total with a worst step of 0.53–1.49 ms. At radius 32, the worst step was 1.17–2.55 ms. This excludes meshing, GPU draws and presentation; concurrent builds make these timings noisy.
