# termexo-fleet

Run **Termexo Fleet** — the local-first AI coding fleet commander for orchestrating one or more [Termexo](https://github.com/gemron/Termexo) desktops — directly from npm on **Windows (x64)**, **macOS (Apple Silicon & Intel)**, and **Linux (x64)**.

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

All platform desktop payloads are bundled directly inside the npm package — no secondary GitHub download is needed on first launch, and npm mirrors (such as `npmmirror`) work out of the box.

> **Linux note:** Debian/Ubuntu systems require GTK 3 / WebKitGTK 4.1 runtime libraries:
> `sudo apt-get install libwebkit2gtk-4.1-0 libgtk-3-0t64 libayatana-appindicator3-1 libsecret-1-0 gnome-keyring fontconfig xdg-utils`

Documentation & source: <https://github.com/gemron/Termexo-Fleet>
