# Store listing artwork

Use the existing catalogue schema on desktop and Android.

- Put the main cover in `preview`. New plugin listings in ProCraft-managed catalogues require one publisher-supplied static PNG, JPEG, or WebP image. A 4:3 image at 1200 × 900 pixels fits the Store well. The Store accepts up to 4 MiB for a cover.
- Add an optional `icon` URL for square artwork on compact Store rows and item headers. It uses the same static formats and 4 MiB limit as the cover. The Store draws square plugin artwork when the listing leaves the icon out.
- Put supplemental images and demos in `previews`, as an ordered array of direct HTTPS URLs. The Store shows the cover first, followed by up to 132 gallery entries. Still images, GIF, APNG, and animated WebP play in the image gallery. GIF, APNG, and animated WebP have a 12 MiB download limit, a 2048 × 2048 frame bound, and a 12-second, 120-frame limit. High-resolution animations also share a 160-million-source-pixel decode budget.
- Use direct `.webm` or `.mp4` URLs for video, up to 25 MiB per file. Playback starts when selected by the viewer. Android plays video on the item page. Desktop opens a local browser player with the already downloaded video. Keep the clip short enough to show the action clearly.
- A bundled, older, or third-party plugin can still appear without publisher artwork. An empty, animated, or unreadable cover uses one of ten locally drawn title-card layouts. The plugin ID keeps the choice stable across both apps and between catalogue refreshes.

For example, include these fields alongside the normal catalogue item metadata.

```json
{
  "preview": "https://example.com/ink-tools/cover.png",
  "icon": "https://example.com/ink-tools/icon.png",
  "previews": [
    "https://example.com/ink-tools/strokes.apng",
    "https://example.com/ink-tools/demo.webm"
  ]
}
```

The preview limits apply to gallery media. The catalogue lists the downloadable pack or plugin separately in `file`, `bytes`, and `sha256`.

## Store Package contents pages

Choose **Shared file** in Store Package for an existing `.brush`, `.brushset`, `.brushlibrary`, `.asset`, `.assetset`, or `.assetlibrary`. The package keeps the original share file and creates gallery pages from its saved thumbnails. Layered assets use the composite thumbnail stored with each asset. Live brush sets and asset files offer matching landscape and square layouts. Choose an independent icon layout to use a different template. Use **Cover image**, **Square icon**, or **Add gallery images** to choose from the clipboard, the current project, a file, or the Android Photo Library. Existing brush files can use custom covers and icons.

The editor shows the generated contents pages when you open the package. Remove the pages you want to leave out and add up to four of your own images. The export lists the selected pages in order before your own images. Upload the cover and square icon separately as `preview` and `icon`. Pages show up to nine items, including group names for library files. Store Package supports up to 1,152 items. A single item fills its own page.

Use `brush` or `brushlibrary` for those file types, `brushset` for brush sets, and `asset pack` for the three asset file types. The Store can display all of these listings. In-Store installation is available for brush sets and plugins.
