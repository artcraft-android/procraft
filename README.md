# Make2D

A painting and 2D animation app for iPad, Android and the desktop, built on a Rust engine and a wgpu renderer.

Make2D is a fork of [ProCraft](https://github.com/artcraft-android/procraft) by the ArtCraft Team and the ProCraft contributors. It keeps ProCraft's core (the brush engine, documents, renderer and plugins) and brings over the feature set of Koi Studio: a track-based timeline with keyframes, live layer effects, audio and video, and the exporters that go with them.

ProCraft's original README is kept at [docs/procraft-README.md](docs/procraft-README.md); its build instructions still apply, with the names changed:

| ProCraft | Make2D |
|---|---|
| `procraft-engine`, `procraft-gpu`, `procraft-core` crates | `make2d-engine`, `make2d-gpu`, `make2d-core` |
| `procraft-desktop` (binary `procraft`) | `make2d-desktop` (binary `make2d`) |
| `ios/ProCraft`, Swift module `ProCraftCore` | `ios/Make2D`, `Make2DCore` |
| Android package and app ID `io.github.artcraftandroid.procraft` | `com.make2d.app` |
| iPad bundle ID `io.github.artcraftandroid.procraft` | `com.make2d.app` |

## Still shared with ProCraft

These keep ProCraft's names on purpose, so canvases and plugins move between the two apps:

- Canvas files: the `.procraft` extension and the `procraft-artwork` format inside them.
- Plugins: `.procraft-plugin` files, the `procraft-plugin-kit` crate, the `procraft` WASM import module and `procraft_*` hooks, and the `compat.procraft` manifest key.
- The app icon and artwork in `assets/` are still ProCraft's and need replacing before any release.

## Upstream

`procraft-upstream` points at ProCraft on GitHub (fetch only), so fixes can still be merged in.

## License

MIT or Apache-2.0, at your option, as ProCraft is. See [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE) and [NOTICE](NOTICE).
