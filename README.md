<p align="center">
  <img src="assets/umubox.svg" alt="UmuBox" width="88" height="88" />
</p>

<h1 align="center">UmuBox</h1>

<p align="center">
  Native GPU-accelerated Rust + <a href="https://gpui.rs">GPUI</a> Wine/Proton Prefix &amp; Game Manager powered by <code>umu-run</code> — styled in <strong>Kanagawa Dragon</strong> with a cobalt-blue accent and zero Electron overhead.
</p>

---

## Features

- **Wine / Proton Prefix Manager:**
  - Automatically discovers existing prefixes and lets you create isolated prefixes under `~/Games/umu` (configurable).
  - Shows prefix disk usage, live `/proc`-detected running Wine/Proton processes, default UMU `GAMEID`, per-prefix Proton override, shader cache cleanup, one-click `wineserver -k` force-kill, and quick file manager reveal.
  - Integrated prefix tools: `winecfg`, `regedit`, `taskmgr`, `cmd`, `wineboot`, and `winetricks`.
- **Saved Games & Per-Game Launch Options:**
  - Add `.exe`, `.msi`, or `.bat` targets with custom arguments, working directory, UMU `GAMEID`, and per-game Proton overrides.
  - Per-prefix and per-game toggles for **MangoHud**, **NTSync** (auto-detects `/dev/ntsync`), **NVAPI** (`PROTON_ENABLE_NVAPI`), **DXVK HUD**, **NVIDIA PRIME dGPU offload**, experimental **Proton Wayland driver** (`PROTON_ENABLE_WAYLAND=1`), configurable **Shader Cache** limit (default `10` GB), `WINEDLLOVERRIDES`, and custom environment variables.
- **Automatic `.exe` Icon Extraction & KDE App Menu Integration:**
  - Extracts the highest-resolution icon directly from Windows PE binaries (`.exe` / `.dll`) or `.ico` files to cached PNGs, with support for custom icon overrides.
  - Creates and syncs managed `.desktop` entries in `~/.local/share/applications` (including desktop actions to **Launch without MangoHud Overlay** or **Force Stop Prefix**), and refreshes `update-desktop-database` and `kbuildsycoca6`.
- **Built-in GE-Proton Manager:**
  - Discovers **User** (`~/.local/share/Steam/compatibilitytools.d`), **System / AUR** (`/usr/share/steam/compatibilitytools.d`), and **Steam** Proton builds.
  - Checks GitHub for the latest **GE-Proton** releases, downloads with live progress, verifies **SHA-512** checksums before extracting, and lets you remove user-installed builds.
- **UMU Database, Protonfixes & ProtonDB Integration:**
  - Offline-first search over the 1,200+ title `umu-database.csv` bundled inside GE-Proton (with one-click online refresh from Open-Wine-Components) to auto-match `GAMEID`s.
  - Inspects bundled `protonfixes` Python scripts to show an **Auto-Fix** badge and any auto-applied winetricks/DLL overrides before launching.
  - Fetches & caches **ProtonDB** tier ratings (`Platinum` / `Gold` / `Silver` / `Bronze` / `Borked`) for Steam-mapped titles.
- **Automatic DLL Scanner & One-Click Prefix Presets:**
  - Scans PE32/PE32+ import tables (`.exe` and adjacent `.dll`s) for required runtimes (`vcrun2022`, `d3dx9`, `d3dcompiler_47`, `dotnet48`, `faudio`, etc.) and side-by-side mod/hook DLLs (`dinput8.dll`, `winmm.dll`, `version.dll`, etc. → one-click `WINEDLLOVERRIDES`).
  - Includes 4 curated **Prefix Presets** (*Modern DX11/DX12*, *Classic DX9/DX10*, *Mod Managers & .NET*, *Cutscenes & Audio*) running unattended (`winetricks -q`).
- **Live Console Log:** Streams `umu-run` stdout/stderr output in the dedicated **Console** tab.
- **Full Headless CLI:** Run installers or executables directly (`umubox ./setup.exe`), launch saved games, manage prefixes/tools/icons/desktop entries, and install or remove GE-Proton builds from any terminal.

---

## CLI Usage

```text
umubox                                        Launch GUI
umubox run [OPTIONS] <EXE|MSI> [--] [ARGS...] Run executable/installer inside a prefix
umubox launch [--no-mangohud] <GAME>          Launch a saved game (e.g. battlenet or pfx/game)
umubox kill <PREFIX>                          Force-kill wineserver for a prefix
umubox tool <PREFIX> <TOOL> [ARGS...]         Run winecfg | regedit | taskmgr | cmd | wineboot | winetricks
umubox list [all|prefixes|games]              List discovered prefixes, games, and live running PIDs
umubox prefix create <NAME> [--path DIR] [--proton BUILD]
                                              Create and register a new Wine/Proton prefix
umubox desktop list                           List saved games & KDE app menu / icon status
umubox desktop add <GAME> [--icon PATH]       Create & manage KDE App Menu .desktop entry (auto .exe icon)
umubox desktop remove <GAME>                  Remove managed KDE App Menu .desktop entry
umubox desktop sync                           Re-sync icons & .desktop entries for all managed games
umubox icon set <GAME> <PATH>                 Set custom icon override for a game
umubox icon clear <GAME>                      Clear custom icon override and inherit from .exe
umubox icon extract <EXE|ICO> <OUT.png>       Extract highest-res icon from .exe/.dll/.ico to PNG
umubox proton list                            List installed Proton & GE-Proton builds
umubox proton check                           Check GitHub for latest GE-Proton releases
umubox proton install [TAG]                   Download, SHA-512 verify, and install GE-Proton
umubox proton remove <BUILD>                  Remove a user-installed Proton build
umubox lookup <QUERY>                         Search UMU DB, protonfixes, and ProtonDB rating
umubox scan <EXE> [--prefix PFX] [--apply]    Scan PE imports/mod DLLs & install missing winetricks
umubox preset list                            List curated winetricks prefix presets
umubox preset apply <PREFIX> <PRESET_ID>      Apply a prefix preset quietly via winetricks -q
```

---

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Ctrl+1` | Switch to **Prefixes** tab |
| `Ctrl+2` | Switch to **Proton** Manager tab |
| `Ctrl+3` | Switch to **Console** tab |
| `Ctrl+R` | Refresh prefixes, Proton builds, and running processes |

---

## Requirements

- **Runtime:** `umu-run` (`umu-launcher`), `curl`, `tar`
- **Optional:** `mangohud`, `winetricks`, `xdg-open`
- **Build:** Rust toolchain (`rustc` + `cargo`, Edition 2024) and Linux GPUI dependencies (Vulkan, Wayland/X11, Fontconfig)

---

## Build & Install

```bash
git clone https://github.com/vovakaravov/UmuBox.git
cd UmuBox
cargo test
cargo build --release

# Put `umubox` on your PATH
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/umubox" ~/.local/bin/umubox
```

### Optional: Linux `.desktop` Entry (KDE / Wayland)

UmuBox sets its Wayland `app_id` to `umubox` so desktop environments match the taskbar icon cleanly:

```bash
mkdir -p ~/.local/share/icons/hicolor/scalable/apps ~/.local/share/applications
cp assets/umubox.svg ~/.local/share/icons/hicolor/scalable/apps/umubox.svg

cat > ~/.local/share/applications/umubox.desktop <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=UmuBox
GenericName=Proton & UMU Prefix Manager
Comment=Native GPU-accelerated Wine/Proton Prefix & Game Launcher via umu-run
Exec=$HOME/.local/bin/umubox
Icon=$HOME/.local/share/icons/hicolor/scalable/apps/umubox.svg
Terminal=false
Categories=Game;Utility;
Keywords=proton;wine;umu;gaming;prefix;winetricks;ge-proton;
StartupNotify=true
StartupWMClass=umubox
EOF
```

---

## License

[MIT](LICENSE)
