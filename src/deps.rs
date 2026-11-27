use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct PrefixPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub verbs: &'static [&'static str],
    pub dll_overrides: &'static str,
}

pub const PREFIX_PRESETS: &[PrefixPreset] = &[
    PrefixPreset {
        id: "modern-dx",
        name: "Modern DX11 / DX12 Essentials",
        description: "Latest VC++ 2015–2022 redistributable, D3DCompiler_47 shaders, and core system fonts",
        verbs: &["vcrun2022", "d3dcompiler_47", "corefonts"],
        dll_overrides: "",
    },
    PrefixPreset {
        id: "classic-dx9",
        name: "Classic DX9 / DX10 Games",
        description: "Legacy D3DX9, D3DCompiler_43, XACT audio, VC++ 2015, and fonts for 2005–2015 titles",
        verbs: &["vcrun2015", "d3dx9", "d3dcompiler_43", "xact", "corefonts"],
        dll_overrides: "",
    },
    PrefixPreset {
        id: "dotnet-mod",
        name: "Mod Managers & .NET Apps",
        description: "VC++ 2022, .NET Framework 4.8, and fonts for Vortex/MO2/community modding tools",
        verbs: &["vcrun2022", "dotnet48", "corefonts"],
        dll_overrides: "",
    },
    PrefixPreset {
        id: "media-cutscenes",
        name: "Cutscenes & Audio Codecs",
        description: "FAudio, Quartz DirectShow, and LAV Filters for broken in-game FMVs and audio",
        verbs: &["faudio", "quartz", "lavfilters"],
        dll_overrides: "",
    },
];

pub fn find_preset(id: &str) -> Option<&'static PrefixPreset> {
    PREFIX_PRESETS.iter().find(|p| p.id.eq_ignore_ascii_case(id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRecommendation {
    pub verb: String,
    pub label: String,
    pub reason: String,
    pub matched_dlls: Vec<String>,
    pub already_installed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideOverride {
    pub dll_stem: String,
    pub filename: String,
    pub override_spec: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyReport {
    pub target_exe: PathBuf,
    pub scanned_binaries: usize,
    pub imported_dlls: Vec<String>,
    pub recommendations: Vec<DepRecommendation>,
    pub side_by_side_overrides: Vec<SideBySideOverride>,
}

pub fn read_installed_winetricks_verbs(prefix_path: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    for cand in [prefix_path.join("winetricks.log"), prefix_path.join("pfx/winetricks.log")] {
        if let Ok(raw) = fs::read_to_string(&cand) {
            for line in raw.lines() {
                let v = line.trim().to_ascii_lowercase();
                if !v.is_empty() && !v.starts_with('#') {
                    set.insert(v);
                }
            }
        }
    }
    set
}

pub fn prefix_has_system_dll(prefix_path: &Path, dll_name: &str) -> bool {
    let lower = dll_name.to_ascii_lowercase();
    for base in [
        prefix_path.join("drive_c/windows/system32"),
        prefix_path.join("pfx/drive_c/windows/system32"),
        prefix_path.join("drive_c/windows/syswow64"),
        prefix_path.join("pfx/drive_c/windows/syswow64"),
    ] {
        if base.join(&lower).is_file() || base.join(dll_name).is_file() {
            return true;
        }
    }
    false
}

#[derive(Debug, Clone)]
struct PeSection {
    virtual_size: u32,
    virtual_address: u32,
    raw_size: u32,
    raw_offset: u32,
}

fn read_u16_le(buf: &[u8], off: usize) -> Result<u16> {
    let s = buf.get(off..off + 2).context("EOF reading u16")?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32_le(buf: &[u8], off: usize) -> Result<u32> {
    let s = buf.get(off..off + 4).context("EOF reading u32")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rva_to_file_offset(sections: &[PeSection], rva: u32) -> Option<u64> {
    for sec in sections {
        let span = sec.raw_size.max(sec.virtual_size);
        let end = sec.virtual_address.checked_add(span)?;
        if rva >= sec.virtual_address && rva < end {
            let delta = rva - sec.virtual_address;
            if delta < sec.raw_size {
                return Some(sec.raw_offset as u64 + delta as u64);
            }
        }
    }
    None
}

fn read_c_string_at(file: &mut File, offset: u64) -> Option<String> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = [0u8; 160];
    let n = file.read(&mut buf).ok()?;
    let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
    let s = String::from_utf8_lossy(&buf[..end]).trim().to_ascii_lowercase();
    (!s.is_empty()).then_some(s)
}

/// Parses standard & delay-load PE32 / PE32+ Import Tables from a Windows executable or DLL.
pub fn scan_pe_imports(pe_path: &Path) -> Result<Vec<String>> {
    let mut file = File::open(pe_path)
        .with_context(|| format!("open PE binary {}", pe_path.display()))?;

    let mut dos = [0u8; 64];
    file.read_exact(&mut dos).context("read DOS header")?;
    if &dos[0..2] != b"MZ" {
        bail!("Not a Windows PE executable (missing MZ header)");
    }
    let e_lfanew = read_u32_le(&dos, 0x3c)? as u64;
    if e_lfanew > 16 * 1024 * 1024 {
        bail!("Invalid PE header offset");
    }

    file.seek(SeekFrom::Start(e_lfanew))?;
    let mut pe_hdr = [0u8; 24];
    file.read_exact(&mut pe_hdr).context("read COFF header")?;
    if &pe_hdr[0..4] != b"PE\0\0" {
        bail!("Invalid PE signature");
    }

    let num_sections = read_u16_le(&pe_hdr, 6)? as usize;
    let opt_size = read_u16_le(&pe_hdr, 20)? as usize;
    if opt_size < 96 || opt_size > 4096 || num_sections > 128 {
        bail!("Unexpected PE optional header or section count");
    }

    let mut opt_buf = vec![0u8; opt_size];
    file.read_exact(&mut opt_buf).context("read Optional header")?;
    let magic = read_u16_le(&opt_buf, 0)?;
    let (data_dir_off, num_dirs_off) = match magic {
        0x10b => (96usize, 92usize),  // PE32
        0x20b => (112usize, 108usize), // PE32+
        _ => bail!("Unknown PE optional header magic {magic:#x}"),
    };
    let num_dirs = read_u32_le(&opt_buf, num_dirs_off).unwrap_or(0) as usize;

    let read_data_dir = |index: usize| -> Option<(u32, u32)> {
        if index >= num_dirs {
            return None;
        }
        let off = data_dir_off + index * 8;
        if off + 8 > opt_buf.len() {
            return None;
        }
        let rva = read_u32_le(&opt_buf, off).ok()?;
        let size = read_u32_le(&opt_buf, off + 4).ok()?;
        (rva != 0 && size != 0).then_some((rva, size))
    };

    let import_dir = read_data_dir(1);
    let delay_dir = read_data_dir(13);

    let mut sec_buf = vec![0u8; num_sections * 40];
    file.read_exact(&mut sec_buf).context("read section headers")?;
    let mut sections = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let off = i * 40;
        sections.push(PeSection {
            virtual_size: read_u32_le(&sec_buf, off + 8)?,
            virtual_address: read_u32_le(&sec_buf, off + 12)?,
            raw_size: read_u32_le(&sec_buf, off + 16)?,
            raw_offset: read_u32_le(&sec_buf, off + 20)?,
        });
    }

    let mut dlls = HashSet::new();

    if let Some((imp_rva, _)) = import_dir
        && let Some(imp_off) = rva_to_file_offset(&sections, imp_rva)
    {
        for i in 0..256u64 {
            if file.seek(SeekFrom::Start(imp_off + i * 20)).is_err() {
                break;
            }
            let mut desc = [0u8; 20];
            if file.read_exact(&mut desc).is_err() {
                break;
            }
            let ilt_rva = read_u32_le(&desc, 0).unwrap_or(0);
            let name_rva = read_u32_le(&desc, 12).unwrap_or(0);
            let iat_rva = read_u32_le(&desc, 16).unwrap_or(0);
            if ilt_rva == 0 && name_rva == 0 && iat_rva == 0 {
                break;
            }
            if name_rva != 0
                && let Some(name_off) = rva_to_file_offset(&sections, name_rva)
                && let Some(dll) = read_c_string_at(&mut file, name_off)
            {
                dlls.insert(dll);
            }
        }
    }

    if let Some((del_rva, _)) = delay_dir
        && let Some(del_off) = rva_to_file_offset(&sections, del_rva)
    {
        for i in 0..128u64 {
            if file.seek(SeekFrom::Start(del_off + i * 32)).is_err() {
                break;
            }
            let mut desc = [0u8; 32];
            if file.read_exact(&mut desc).is_err() {
                break;
            }
            let gr_attrs = read_u32_le(&desc, 0).unwrap_or(0);
            let sz_name_rva = read_u32_le(&desc, 4).unwrap_or(0);
            if sz_name_rva == 0 {
                break;
            }
            if (gr_attrs & 1) != 0
                && let Some(name_off) = rva_to_file_offset(&sections, sz_name_rva)
                && let Some(dll) = read_c_string_at(&mut file, name_off)
            {
                dlls.insert(dll);
            }
        }
    }

    let mut out: Vec<String> = dlls.into_iter().collect();
    out.sort();
    Ok(out)
}

const KNOWN_MOD_PROXY_DLLS: &[(&str, &str)] = &[
    ("dinput8.dll", "dinput8=n,b"),
    ("winmm.dll", "winmm=n,b"),
    ("version.dll", "version=n,b"),
    ("dxgi.dll", "dxgi=n,b"),
    ("d3d9.dll", "d3d9=n,b"),
    ("d3d11.dll", "d3d11=n,b"),
    ("dsound.dll", "dsound=n,b"),
    ("xinput1_3.dll", "xinput1_3=n,b"),
    ("xinput1_4.dll", "xinput1_4=n,b"),
    ("dbghelp.dll", "dbghelp=n,b"),
    ("bink2w64.dll", "bink2w64=n,b"),
    ("msimg32.dll", "msimg32=n,b"),
    ("winhttp.dll", "winhttp=n,b"),
];

pub fn analyze_exe_dependencies(
    exe_path: &Path,
    prefix_path: Option<&Path>,
) -> Result<DependencyReport> {
    if !exe_path.is_file() {
        bail!("Executable not found: {}", exe_path.display());
    }

    let mut all_imports = HashSet::new();
    let mut scanned = 0usize;
    for dll in scan_pe_imports(exe_path)? {
        all_imports.insert(dll);
    }
    scanned += 1;

    let mut side_by_side_overrides = Vec::new();
    if let Some(dir) = exe_path.parent().filter(|d| d.is_dir())
        && let Ok(entries) = fs::read_dir(dir)
    {
        let mut dll_paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
            })
            .collect();
        dll_paths.sort();

        for dll_p in &dll_paths {
            if let Some(fname) = dll_p.file_name().and_then(|n| n.to_str()) {
                let lower = fname.to_ascii_lowercase();
                if let Some(&(_, spec)) = KNOWN_MOD_PROXY_DLLS
                    .iter()
                    .find(|&&(k, _)| k == lower)
                {
                    let stem = spec.split('=').next().unwrap_or(&lower).to_string();
                    side_by_side_overrides.push(SideBySideOverride {
                        dll_stem: stem,
                        filename: fname.to_string(),
                        override_spec: spec.to_string(),
                        reason: format!(
                            "Side-by-side '{fname}' beside executable — Wine ignores it without `{spec}` override"
                        ),
                    });
                }
            }
        }

        for dll_p in dll_paths.into_iter().take(16) {
            if let Ok(meta) = fs::metadata(&dll_p)
                && meta.len() <= 64 * 1024 * 1024
                && let Ok(imps) = scan_pe_imports(&dll_p)
            {
                scanned += 1;
                all_imports.extend(imps);
            }
        }
    }

    let mut imported_dlls: Vec<String> = all_imports.into_iter().collect();
    imported_dlls.sort();

    let installed_verbs = prefix_path
        .map(read_installed_winetricks_verbs)
        .unwrap_or_default();

    let rules: &[(&str, &str, &str, &[&str])] = &[
        (
            "vcrun2022",
            "Visual C++ 2015–2022 Runtime",
            "Required by VC++ 14.x binaries (`msvcp140` / `vcruntime140`)",
            &["msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll", "concrt140.dll", "vcomp140.dll"],
        ),
        (
            "vcrun2013",
            "Visual C++ 2013 Runtime",
            "Required by VC++ 12.0 binaries (`msvcp120` / `msvcr120`)",
            &["msvcp120.dll", "msvcr120.dll"],
        ),
        (
            "vcrun2012",
            "Visual C++ 2012 Runtime",
            "Required by VC++ 11.0 binaries (`msvcp110` / `msvcr110`)",
            &["msvcp110.dll", "msvcr110.dll"],
        ),
        (
            "vcrun2010",
            "Visual C++ 2010 Runtime",
            "Required by VC++ 10.0 binaries (`msvcp100` / `msvcr100`)",
            &["msvcp100.dll", "msvcr100.dll"],
        ),
        (
            "d3dcompiler_47",
            "D3DCompiler_47 Shader Compiler",
            "Direct3D HLSL runtime shader compilation",
            &["d3dcompiler_47.dll"],
        ),
        (
            "d3dcompiler_43",
            "D3DCompiler_43 Shader Compiler",
            "Legacy DirectX 10/11 shader compiler",
            &["d3dcompiler_43.dll"],
        ),
        (
            "d3dx9",
            "Direct3D 9 Extensions (d3dx9)",
            "Legacy DirectX 9 math/texture helper libraries",
            &["d3dx9_43.dll", "d3dx9_42.dll", "d3dx9_41.dll", "d3dx9_39.dll", "d3dx9_36.dll", "d3dx9_35.dll"],
        ),
        (
            "d3dx10",
            "Direct3D 10 Extensions (d3dx10)",
            "Legacy DirectX 10 helper library",
            &["d3dx10_43.dll", "d3dx10_42.dll"],
        ),
        (
            "d3dx11_43",
            "Direct3D 11 Extensions (d3dx11)",
            "DirectX 11 June 2010 SDK helper library",
            &["d3dx11_43.dll", "d3dx11_42.dll"],
        ),
        (
            "xact",
            "XACT Audio Engine",
            "Microsoft XACT / XAudio legacy soundbanks",
            &["xactengine3_7.dll", "xactengine3_6.dll", "xaudio2_7.dll"],
        ),
        (
            "faudio",
            "FAudio / XAudio2 Runtime",
            "Modern XAudio2 implementation for cutscenes & game audio",
            &["xaudio2_8.dll", "xaudio2_9.dll", "x3daudio1_7.dll"],
        ),
        (
            "dotnet48",
            ".NET Framework 4.8",
            "Required by C# / WPF launchers and mod managers (`mscoree.dll`)",
            &["mscoree.dll"],
        ),
        (
            "openal",
            "OpenAL Audio",
            "3D positional audio library (`openal32.dll`)",
            &["openal32.dll"],
        ),
        (
            "physx",
            "NVIDIA PhysX Legacy",
            "Hardware/CPU PhysX runtime DLLs",
            &["physxloader.dll", "physx3_x64.dll", "physx3_x86.dll"],
        ),
        (
            "directplay",
            "DirectPlay Networking",
            "Legacy LAN / multiplayer lobby support",
            &["dplayx.dll", "dpnet.dll"],
        ),
    ];

    let mut recommendations = Vec::new();
    for &(verb, label, reason, patterns) in rules {
        let matched: Vec<String> = imported_dlls
            .iter()
            .filter(|imp| patterns.iter().any(|p| *p == imp.as_str()))
            .cloned()
            .collect();
        if matched.is_empty() {
            continue;
        }
        let mut installed = installed_verbs.contains(verb);
        if !installed
            && let Some(pfx) = prefix_path
        {
            installed = matched.iter().all(|dll| prefix_has_system_dll(pfx, dll));
        }
        recommendations.push(DepRecommendation {
            verb: verb.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
            matched_dlls: matched,
            already_installed: installed,
        });
    }

    Ok(DependencyReport {
        target_exe: exe_path.to_path_buf(),
        scanned_binaries: scanned,
        imported_dlls,
        recommendations,
        side_by_side_overrides,
    })
}

pub fn merge_dll_overrides(existing: &str, additions: &[String]) -> String {
    let mut parts: Vec<String> = existing
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect();

    for add in additions {
        let trimmed = add.trim();
        if trimmed.is_empty() {
            continue;
        }
        let stem = trimmed
            .split('=')
            .next()
            .unwrap_or(trimmed)
            .trim()
            .to_ascii_lowercase();
        if let Some(pos) = parts.iter().position(|p| {
            p.split('=')
                .next()
                .unwrap_or(p)
                .trim()
                .eq_ignore_ascii_case(&stem)
        }) {
            parts[pos] = trimmed.to_string();
        } else {
            parts.push(trimmed.to_string());
        }
    }

    parts.join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_minimal_pe64_with_imports(dll_names: &[&str]) -> Vec<u8> {
        // Minimal valid PE32+ image with 1 `.idata` section at RVA 0x1000, file offset 0x200.
        let mut buf = vec![0u8; 0x1000];
        buf[0..2].copy_from_slice(b"MZ");
        buf[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());

        let pe = 0x80usize;
        buf[pe..pe + 4].copy_from_slice(b"PE\0\0");
        buf[pe + 6..pe + 8].copy_from_slice(&1u16.to_le_bytes()); // 1 section
        buf[pe + 20..pe + 22].copy_from_slice(&240u16.to_le_bytes()); // SizeOfOptionalHeader (PE32+)

        let opt = pe + 24;
        buf[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes()); // PE32+ magic
        buf[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes()); // NumberOfRvaAndSizes
        // DataDirectory[1] (Import Table) at opt + 112 + 8 = opt + 120
        buf[opt + 120..opt + 124].copy_from_slice(&0x1000u32.to_le_bytes());
        buf[opt + 124..opt + 128].copy_from_slice(&256u32.to_le_bytes());

        let sec = opt + 240;
        buf[sec..sec + 8].copy_from_slice(b".idata\0\0");
        buf[sec + 8..sec + 12].copy_from_slice(&0x400u32.to_le_bytes()); // VirtualSize
        buf[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes()); // VirtualAddress
        buf[sec + 16..sec + 20].copy_from_slice(&0x400u32.to_le_bytes()); // SizeOfRawData
        buf[sec + 20..sec + 24].copy_from_slice(&0x400u32.to_le_bytes()); // PointerToRawData

        let desc_base = 0x400usize;
        let mut str_cursor = desc_base + (dll_names.len() + 1) * 20;
        for (idx, name) in dll_names.iter().enumerate() {
            let d = desc_base + idx * 20;
            let name_rva = (0x1000 + (str_cursor - desc_base)) as u32;
            buf[d..d + 4].copy_from_slice(&1u32.to_le_bytes()); // non-zero ILT marker
            buf[d + 12..d + 16].copy_from_slice(&name_rva.to_le_bytes());
            let bytes = name.as_bytes();
            buf[str_cursor..str_cursor + bytes.len()].copy_from_slice(bytes);
            buf[str_cursor + bytes.len()] = 0;
            str_cursor += bytes.len() + 1;
        }
        buf
    }

    #[test]
    fn scans_pe64_imports_and_side_by_side_mod_dlls() {
        let tmp = tempfile::tempdir().unwrap();
        let game_dir = tmp.path().join("MyGame");
        let pfx_dir = tmp.path().join("pfx");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&pfx_dir).unwrap();
        fs::write(pfx_dir.join("winetricks.log"), "d3dcompiler_47\n").unwrap();

        let exe = game_dir.join("game.exe");
        let pe = build_minimal_pe64_with_imports(&[
            "VCRUNTIME140_1.dll",
            "d3dx9_43.dll",
            "d3dcompiler_47.dll",
        ]);
        fs::write(&exe, pe).unwrap();
        fs::write(game_dir.join("dinput8.dll"), b"mod loader stub").unwrap();

        let report = analyze_exe_dependencies(&exe, Some(&pfx_dir)).unwrap();
        assert!(report.imported_dlls.contains(&"vcruntime140_1.dll".into()));
        assert!(report.imported_dlls.contains(&"d3dx9_43.dll".into()));

        let vc = report
            .recommendations
            .iter()
            .find(|r| r.verb == "vcrun2022")
            .unwrap();
        assert!(!vc.already_installed);

        let d3dc = report
            .recommendations
            .iter()
            .find(|r| r.verb == "d3dcompiler_47")
            .unwrap();
        assert!(d3dc.already_installed);

        assert_eq!(report.side_by_side_overrides.len(), 1);
        assert_eq!(report.side_by_side_overrides[0].override_spec, "dinput8=n,b");
    }

    #[test]
    fn merges_dll_overrides_without_duplicate_stems() {
        let merged = merge_dll_overrides(
            "locationapi=d;winmm=b",
            &["winmm=n,b".into(), "dinput8=n,b".into()],
        );
        assert_eq!(merged, "locationapi=d;winmm=n,b;dinput8=n,b");
    }
}
