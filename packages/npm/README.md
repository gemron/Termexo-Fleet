# termexo-fleet

Local-first AI coding fleet commander for orchestrating one or more [Termexo](https://github.com/gemron/Termexo) desktops across **Windows**, **macOS**, and **Linux**.

## Quick Start (Node.js 18.18+)

Run immediately without permanent installation:

```bash
npx termexo-fleet@latest
```

Or install globally:

```bash
npm install --global termexo-fleet
termexo-fleet
```

On first launch, this CLI downloads the matching prebuilt desktop binary from [GitHub Releases](https://github.com/gemron/Termexo-Fleet/releases), verifies its SHA-256 checksum against the embedded release manifest, caches it locally, and opens Termexo Fleet.

> **Linux note:** Debian/Ubuntu systems require WebKitGTK/GTK runtime libraries:
> `sudo apt install libwebkit2gtk-4.1-0 libgtk-3-0 libayatana-appindicator3-1`

Full documentation: <https://github.com/gemron/Termexo-Fleet>
