use crate::config::GameEntry;
use anyhow::{Context, Result, anyhow, bail};
use gpui_kit::{Image, ImageFormat};
use image::ImageReader;
use image::imageops::FilterType;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

const PNG_SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
const MAX_RSRC_BYTES: usize = 64 * 1024 * 1024;
const MAX_RSRC_DEPTH: usize = 6;

pub fn icons_cache_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("umubox")
        .join("icons")
}

pub fn icon_key(prefix_id: &str, game_id: &str) -> String {
    format!("{prefix_id}-{game_id}")
}

pub fn cached_icon_path_in(cache_dir: &Path, key: &str) -> Option<PathBuf> {
    let svg = cache_dir.join(format!("{key}.svg"));
    if svg.is_file() {
        return Some(svg);
    }
    let png = cache_dir.join(format!("{key}.png"));
    if png.is_file() {
        return Some(png);
    }
    None
}

pub fn cached_icon_path(prefix_id: &str, game_id: &str) -> Option<PathBuf> {
    cached_icon_path_in(&icons_cache_dir(), &icon_key(prefix_id, game_id))
}

pub fn remove_cached_icon_in(cache_dir: &Path, key: &str) {
    let _ = fs::remove_file(cache_dir.join(format!("{key}.png")));
    let _ = fs::remove_file(cache_dir.join(format!("{key}.svg")));
}

pub fn remove_cached_icon(prefix_id: &str, game_id: &str) {
    remove_cached_icon_in(&icons_cache_dir(), &icon_key(prefix_id, game_id));
}

#[derive(Debug, Clone)]
struct PeSection {
    virtual_size: u32,
    virtual_address: u32,
    raw_size: u32,
    raw_offset: u32,
}

fn read_u16_le(buf: &[u8], off: usize) -> Result<u16> {
    let slice = buf
        .get(off..off + 2)
        .ok_or_else(|| anyhow!("unexpected EOF reading u16 at {off}"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(buf: &[u8], off: usize) -> Result<u32> {
    let slice = buf
        .get(off..off + 4)
        .ok_or_else(|| anyhow!("unexpected EOF reading u32 at {off}"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u32_be(buf: &[u8], off: usize) -> Result<u32> {
    let slice = buf
        .get(off..off + 4)
        .ok_or_else(|| anyhow!("unexpected EOF reading u32be at {off}"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn rva_to_file_offset(sections: &[PeSection], rva: u32) -> Option<u64> {
    for sec in sections {
        let span = sec.raw_size.max(sec.virtual_size);
        let Some(end) = sec.virtual_address.checked_add(span) else {
            continue;
        };
        if rva >= sec.virtual_address && rva < end {
            let delta = rva - sec.virtual_address;
            if delta < sec.raw_size {
                return Some(sec.raw_offset as u64 + delta as u64);
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
struct RsrcDirEntry {
    id_or_name: u32,
    is_named: bool,
    is_dir: bool,
    offset: usize,
}

fn parse_rsrc_dir(rsrc: &[u8], dir_off: usize) -> Result<Vec<RsrcDirEntry>> {
    if dir_off + 16 > rsrc.len() {
        bail!("PE resource directory out of bounds at {dir_off}");
    }
    let num_named = read_u16_le(rsrc, dir_off + 12)? as usize;
    let num_id = read_u16_le(rsrc, dir_off + 14)? as usize;
    let total = num_named.saturating_add(num_id);
    if dir_off + 16 + total.saturating_mul(8) > rsrc.len() {
        bail!("PE resource directory entries out of bounds at {dir_off}");
    }

    let mut out = Vec::with_capacity(total);
    for i in 0..total {
        let pos = dir_off + 16 + i * 8;
        let name_field = read_u32_le(rsrc, pos)?;
        let data_field = read_u32_le(rsrc, pos + 4)?;
        out.push(RsrcDirEntry {
            id_or_name: name_field & 0x7fff_ffff,
            is_named: (name_field & 0x8000_0000) != 0,
            is_dir: (data_field & 0x8000_0000) != 0,
            offset: (data_field & 0x7fff_ffff) as usize,
        });
    }
    Ok(out)
}

fn collect_first_data_leaf(rsrc: &[u8], off: usize, is_dir: bool, depth: usize) -> Option<(u32, u32)> {
    if depth >= MAX_RSRC_DEPTH {
        return None;
    }
    if !is_dir {
        if off + 16 > rsrc.len() {
            return None;
        }
        let data_rva = read_u32_le(rsrc, off).ok()?;
        let data_size = read_u32_le(rsrc, off + 4).ok()?;
        return Some((data_rva, data_size));
    }
    let entries = parse_rsrc_dir(rsrc, off).ok()?;
    for entry in entries {
        if let Some(leaf) = collect_first_data_leaf(rsrc, entry.offset, entry.is_dir, depth + 1) {
            return Some(leaf);
        }
    }
    None
}

fn read_rsrc_data(
    file: &mut File,
    sections: &[PeSection],
    rsrc: &[u8],
    rsrc_rva: u32,
    data_rva: u32,
    data_size: u32,
) -> Result<Vec<u8>> {
    if data_size == 0 || data_size as usize > 16 * 1024 * 1024 {
        bail!("unreasonable PE resource data size: {data_size}");
    }
    let sz = data_size as usize;
    if data_rva >= rsrc_rva {
        let rel = (data_rva - rsrc_rva) as usize;
        if let Some(slice) = rsrc.get(rel..rel.saturating_add(sz)) {
            return Ok(slice.to_vec());
        }
    }
    let file_off = rva_to_file_offset(sections, data_rva)
        .ok_or_else(|| anyhow!("cannot map PE resource RVA {data_rva:#x} to file offset"))?;
    file.seek(SeekFrom::Start(file_off))?;
    let mut buf = vec![0u8; sz];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

fn png_dimensions_if_png(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(PNG_SIG) || bytes.len() < 24 {
        return None;
    }
    let w = read_u32_be(bytes, 16).ok()?;
    let h = read_u32_be(bytes, 20).ok()?;
    Some((w, h))
}

fn encode_dynamic_image_png(img: image::DynamicImage) -> Result<Vec<u8>> {
    let w = img.width();
    let h = img.height();
    let prepared = if w.max(h) > 512 {
        img.resize(256, 256, FilterType::Lanczos3)
    } else {
        img
    };
    let mut out = Vec::new();
    prepared
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .context("encode normalized PNG icon")?;
    Ok(out)
}

fn normalize_icon_frame_bytes_to_png(raw: &[u8], w_hint: u8, h_hint: u8, bpp_hint: u16, color_count: u8) -> Result<Vec<u8>> {
    if raw.starts_with(PNG_SIG) {
        let decoded = image::load_from_memory_with_format(raw, image::ImageFormat::Png)
            .context("decode PNG icon frame")?;
        return encode_dynamic_image_png(decoded);
    }

    if raw.len() < 40 {
        bail!("DIB icon frame too small ({} bytes)", raw.len());
    }

    // Wrap single DIB frame in a minimal 1-entry .ico container so `image` decodes BITMAPINFOHEADER + XOR/AND masks.
    let mut ico = Vec::with_capacity(22 + raw.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico.push(w_hint);
    ico.push(h_hint);
    ico.push(color_count);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    let bpp = if bpp_hint > 0 { bpp_hint } else { 32 };
    ico.extend_from_slice(&bpp.to_le_bytes());
    ico.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(raw);

    let decoded = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico)
        .context("decode DIB icon frame")?;
    encode_dynamic_image_png(decoded)
}

fn decode_dib_header_hint(raw: &[u8]) -> Option<(u8, u8, u16)> {
    if raw.len() < 16 {
        return None;
    }
    let w = read_u32_le(raw, 4).ok()?;
    let h_dbl = read_u32_le(raw, 8).ok()? as i32;
    let h = (h_dbl.unsigned_abs() / 2).max(1);
    let bpp = read_u16_le(raw, 14).ok().unwrap_or(32);
    let wh = if w >= 256 || w == 0 { 0u8 } else { w as u8 };
    let hh = if h >= 256 || h == 0 { 0u8 } else { h as u8 };
    Some((wh, hh, bpp))
}

pub fn extract_best_icon_png_from_pe(pe_path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(pe_path)
        .with_context(|| format!("open executable {}", pe_path.display()))?;

    let mut dos = [0u8; 64];
    file.read_exact(&mut dos)
        .with_context(|| format!("read DOS header of {}", pe_path.display()))?;
    if &dos[0..2] != b"MZ" {
        bail!("{} is not a Windows PE executable (missing MZ signature)", pe_path.display());
    }
    let e_lfanew = read_u32_le(&dos, 60)? as u64;
    file.seek(SeekFrom::Start(e_lfanew))?;

    let mut pe_hdr = [0u8; 24];
    file.read_exact(&mut pe_hdr)?;
    if &pe_hdr[0..4] != b"PE\0\0" {
        bail!("{} has invalid PE signature", pe_path.display());
    }
    let num_sections = read_u16_le(&pe_hdr, 6)? as usize;
    let opt_size = read_u16_le(&pe_hdr, 20)? as usize;
    if opt_size < 120 {
        bail!("PE optional header too small ({opt_size} bytes)");
    }

    let mut opt = vec![0u8; opt_size];
    file.read_exact(&mut opt)?;
    let magic = read_u16_le(&opt, 0)?;
    let (num_rva_off, dd_off) = match magic {
        0x010b => (92usize, 96usize),  // PE32
        0x020b => (108usize, 112usize), // PE32+
        _ => bail!("Unsupported PE optional header magic: {magic:#x}"),
    };
    let num_rvas = read_u32_le(&opt, num_rva_off)?;
    if num_rvas <= 2 {
        bail!("PE executable has no resource data directory");
    }
    let rsrc_rva = read_u32_le(&opt, dd_off + 2 * 8)?;
    let rsrc_size = read_u32_le(&opt, dd_off + 2 * 8 + 4)?;
    if rsrc_rva == 0 || rsrc_size == 0 {
        bail!("PE executable contains no .rsrc resources");
    }

    let mut sec_buf = vec![0u8; num_sections.saturating_mul(40)];
    file.read_exact(&mut sec_buf)?;
    let mut sections = Vec::with_capacity(num_sections);
    for i in 0..num_sections {
        let base = i * 40;
        sections.push(PeSection {
            virtual_size: read_u32_le(&sec_buf, base + 8)?,
            virtual_address: read_u32_le(&sec_buf, base + 12)?,
            raw_size: read_u32_le(&sec_buf, base + 16)?,
            raw_offset: read_u32_le(&sec_buf, base + 20)?,
        });
    }

    let rsrc_file_off = rva_to_file_offset(&sections, rsrc_rva)
        .ok_or_else(|| anyhow!("cannot map PE .rsrc RVA {rsrc_rva:#x} to file offset"))?;
    let rsrc_read_len = (rsrc_size as usize).min(MAX_RSRC_BYTES);
    file.seek(SeekFrom::Start(rsrc_file_off))?;
    let mut rsrc = vec![0u8; rsrc_read_len];
    file.read_exact(&mut rsrc)
        .with_context(|| format!("read PE .rsrc section from {}", pe_path.display()))?;

    let root = parse_rsrc_dir(&rsrc, 0)?;
    let rt_icon = root
        .iter()
        .find(|e| !e.is_named && e.id_or_name == 3 && e.is_dir);
    let rt_group_icon = root
        .iter()
        .find(|e| !e.is_named && e.id_or_name == 14 && e.is_dir);

    let Some(icon_dir_entry) = rt_icon else {
        bail!("PE executable contains no RT_ICON resources");
    };

    let mut icons_by_id: HashMap<u16, (u32, u32)> = HashMap::new();
    for entry in parse_rsrc_dir(&rsrc, icon_dir_entry.offset)? {
        if !entry.is_named
            && let Ok(id) = u16::try_from(entry.id_or_name)
            && let Some(leaf) = collect_first_data_leaf(&rsrc, entry.offset, entry.is_dir, 1)
        {
            icons_by_id.insert(id, leaf);
        }
    }
    if icons_by_id.is_empty() {
        bail!("PE executable RT_ICON directory is empty");
    }

    if let Some(group_dir_entry) = rt_group_icon {
        let mut group_entries = parse_rsrc_dir(&rsrc, group_dir_entry.offset)?;
        // Windows Explorer uses the first named group or lowest numeric ID group as the main app icon.
        group_entries.sort_by_key(|e| (!e.is_named, e.id_or_name));

        for grp in group_entries {
            let Some((grp_rva, grp_sz)) =
                collect_first_data_leaf(&rsrc, grp.offset, grp.is_dir, 1)
            else {
                continue;
            };
            let Ok(grp_bytes) =
                read_rsrc_data(&mut file, &sections, &rsrc, rsrc_rva, grp_rva, grp_sz)
            else {
                continue;
            };
            if grp_bytes.len() < 6 {
                continue;
            }
            let reserved = read_u16_le(&grp_bytes, 0).unwrap_or(1);
            let res_type = read_u16_le(&grp_bytes, 2).unwrap_or(0);
            let count = read_u16_le(&grp_bytes, 4).unwrap_or(0) as usize;
            if reserved != 0 || res_type != 1 {
                continue;
            }

            #[derive(Clone)]
            struct FrameCandidate {
                score: (u32, u16, u32),
                w_hint: u8,
                h_hint: u8,
                bpp_hint: u16,
                color_count: u8,
                raw: Vec<u8>,
            }

            let mut candidates: Vec<FrameCandidate> = Vec::new();
            for i in 0..count {
                let off = 6 + i * 14;
                if off + 14 > grp_bytes.len() {
                    break;
                }
                let w_hint = grp_bytes[off];
                let h_hint = grp_bytes[off + 1];
                let color_count = grp_bytes[off + 2];
                let bpp_raw = read_u16_le(&grp_bytes, off + 6).unwrap_or(32);
                let bytes_in_res = read_u32_le(&grp_bytes, off + 8).unwrap_or(0);
                let n_id = read_u16_le(&grp_bytes, off + 12).unwrap_or(0);

                let Some(&(icon_rva, icon_sz)) = icons_by_id.get(&n_id) else {
                    continue;
                };
                let Ok(raw) =
                    read_rsrc_data(&mut file, &sections, &rsrc, rsrc_rva, icon_rva, icon_sz)
                else {
                    continue;
                };

                let (eff_w, eff_h, eff_bpp) = if let Some((pw, ph)) = png_dimensions_if_png(&raw) {
                    (pw, ph, 32u16)
                } else {
                    let ew = if w_hint == 0 { 256 } else { w_hint as u32 };
                    let eh = if h_hint == 0 { 256 } else { h_hint as u32 };
                    let bpp = if bpp_raw == 0 { 32 } else { bpp_raw };
                    (ew, eh, bpp)
                };

                candidates.push(FrameCandidate {
                    score: (eff_w.max(eff_h), eff_bpp, bytes_in_res.max(icon_sz)),
                    w_hint,
                    h_hint,
                    bpp_hint: eff_bpp,
                    color_count,
                    raw,
                });
            }

            candidates.sort_by(|a, b| b.score.cmp(&a.score));
            for cand in candidates {
                if let Ok(png) = normalize_icon_frame_bytes_to_png(
                    &cand.raw,
                    cand.w_hint,
                    cand.h_hint,
                    cand.bpp_hint,
                    cand.color_count,
                ) {
                    return Ok(png);
                }
            }
        }
    }

    // Fallback when RT_GROUP_ICON is missing or malformed: try RT_ICON entries largest-first.
    let mut raw_icons: Vec<(u16, (u32, u32))> = icons_by_id.into_iter().collect();
    raw_icons.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    for (_, (rva, sz)) in raw_icons {
        let Ok(raw) = read_rsrc_data(&mut file, &sections, &rsrc, rsrc_rva, rva, sz) else {
            continue;
        };
        if raw.starts_with(PNG_SIG)
            && let Ok(png) = normalize_icon_frame_bytes_to_png(&raw, 0, 0, 32, 0)
        {
            return Ok(png);
        }
        if let Some((wh, hh, bpp)) = decode_dib_header_hint(&raw)
            && let Ok(png) = normalize_icon_frame_bytes_to_png(&raw, wh, hh, bpp, 0)
        {
            return Ok(png);
        }
    }

    bail!("Failed to decode any icon image resource in {}", pe_path.display())
}

pub fn decode_best_ico_to_png(ico_bytes: &[u8]) -> Result<Vec<u8>> {
    if ico_bytes.len() >= 6 {
        let reserved = read_u16_le(ico_bytes, 0).unwrap_or(1);
        let res_type = read_u16_le(ico_bytes, 2).unwrap_or(0);
        let count = read_u16_le(ico_bytes, 4).unwrap_or(0) as usize;
        if reserved == 0 && res_type == 1 && count > 0 {
            struct IcoCand<'a> {
                score: (u32, u16, u32),
                w_hint: u8,
                h_hint: u8,
                bpp: u16,
                color_count: u8,
                slice: &'a [u8],
            }
            let mut candidates = Vec::new();
            for i in 0..count {
                let pos = 6 + i * 16;
                if pos + 16 > ico_bytes.len() {
                    break;
                }
                let w_hint = ico_bytes[pos];
                let h_hint = ico_bytes[pos + 1];
                let color_count = ico_bytes[pos + 2];
                let bpp = read_u16_le(ico_bytes, pos + 6).unwrap_or(32);
                let sz = read_u32_le(ico_bytes, pos + 8).unwrap_or(0);
                let off = read_u32_le(ico_bytes, pos + 12).unwrap_or(0) as usize;
                let Some(slice) = ico_bytes.get(off..off.saturating_add(sz as usize)) else {
                    continue;
                };
                let (ew, eh, ebpp) = if let Some((pw, ph)) = png_dimensions_if_png(slice) {
                    (pw, ph, 32u16)
                } else {
                    (
                        if w_hint == 0 { 256 } else { w_hint as u32 },
                        if h_hint == 0 { 256 } else { h_hint as u32 },
                        if bpp == 0 { 32 } else { bpp },
                    )
                };
                candidates.push(IcoCand {
                    score: (ew.max(eh), ebpp, sz),
                    w_hint,
                    h_hint,
                    bpp: ebpp,
                    color_count,
                    slice,
                });
            }
            candidates.sort_by(|a, b| b.score.cmp(&a.score));
            for cand in candidates {
                if let Ok(png) = normalize_icon_frame_bytes_to_png(
                    cand.slice,
                    cand.w_hint,
                    cand.h_hint,
                    cand.bpp,
                    cand.color_count,
                ) {
                    return Ok(png);
                }
            }
        }
    }

    let decoded = image::load_from_memory_with_format(ico_bytes, image::ImageFormat::Ico)
        .context("decode .ico image")?;
    encode_dynamic_image_png(decoded)
}

fn source_modified_time(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

fn atomic_write_bytes(dest: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create icon cache dir {}", parent.display()))?;
    }
    let tmp = dest.with_extension("umubox-tmp");
    fs::write(&tmp, bytes).with_context(|| format!("write temp icon {}", tmp.display()))?;
    if fs::rename(&tmp, dest).is_err() {
        let _ = fs::remove_file(&tmp);
        fs::write(dest, bytes).with_context(|| format!("write icon {}", dest.display()))?;
    }
    Ok(())
}

fn materialize_source_bytes(src: &Path) -> Result<(bool, Vec<u8>)> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    match ext.as_str() {
        "svg" => {
            let bytes = fs::read(src).with_context(|| format!("read SVG icon {}", src.display()))?;
            Ok((true, bytes))
        }
        "exe" | "dll" => {
            let png = extract_best_icon_png_from_pe(src)?;
            Ok((false, png))
        }
        "ico" => {
            let bytes = fs::read(src).with_context(|| format!("read ICO file {}", src.display()))?;
            let png = decode_best_ico_to_png(&bytes)?;
            Ok((false, png))
        }
        _ => {
            let decoded = ImageReader::open(src)
                .with_context(|| format!("open image {}", src.display()))?
                .with_guessed_format()
                .context("guess image format")?
                .decode()
                .with_context(|| format!("decode image {}", src.display()))?;
            let png = encode_dynamic_image_png(decoded)?;
            Ok((false, png))
        }
    }
}

pub fn sync_game_icon_in(
    cache_dir: &Path,
    key: &str,
    exe_path: &Path,
    icon_override: Option<&Path>,
    force_refresh: bool,
) -> Result<Option<PathBuf>> {
    let (source, is_override) = match icon_override {
        Some(ov) => (ov, true),
        None => {
            if !exe_path.is_file() {
                remove_cached_icon_in(cache_dir, key);
                return Ok(None);
            }
            (exe_path, false)
        }
    };

    if is_override && !source.is_file() {
        bail!("Custom icon file not found: {}", source.display());
    }

    if !force_refresh
        && let Some(cached) = cached_icon_path_in(cache_dir, key)
        && let (Some(src_m), Some(dst_m)) =
            (source_modified_time(source), source_modified_time(&cached))
        && dst_m >= src_m
    {
        return Ok(Some(cached));
    }

    match materialize_source_bytes(source) {
        Ok((is_svg, bytes)) => {
            let target = cache_dir.join(format!("{key}.{}", if is_svg { "svg" } else { "png" }));
            let stale = cache_dir.join(format!("{key}.{}", if is_svg { "png" } else { "svg" }));
            atomic_write_bytes(&target, &bytes)?;
            let _ = fs::remove_file(stale);
            Ok(Some(target))
        }
        Err(err) => {
            if is_override {
                Err(err)
            } else {
                remove_cached_icon_in(cache_dir, key);
                Ok(None)
            }
        }
    }
}

pub fn sync_game_icon(
    prefix_id: &str,
    game: &GameEntry,
    force_refresh: bool,
) -> Result<Option<PathBuf>> {
    sync_game_icon_in(
        &icons_cache_dir(),
        &icon_key(prefix_id, &game.id),
        &game.exe_path,
        game.icon_override.as_deref(),
        force_refresh,
    )
}

pub fn load_gpui_image_from_cached(path: &Path) -> Option<Arc<Image>> {
    let bytes = fs::read(path).ok()?;
    let fmt = if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        ImageFormat::Svg
    } else {
        ImageFormat::Png
    };
    Some(Arc::new(Image::from_bytes(fmt, bytes)))
}

pub fn load_gpui_preview_for_source(source: &Path) -> Option<Arc<Image>> {
    if !source.is_file() {
        return None;
    }
    let (is_svg, bytes) = materialize_source_bytes(source).ok()?;
    let fmt = if is_svg {
        ImageFormat::Svg
    } else {
        ImageFormat::Png
    };
    Some(Arc::new(Image::from_bytes(fmt, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn make_rgba_png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |x, y| Rgba([(x * 7) as u8, (y * 11) as u8, 200, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn build_synthetic_pe_with_rsrc(icon_payloads: &[(u16, Vec<u8>)], grp_entries: &[(u8, u8, u16, u16)]) -> Vec<u8> {
        // Construct minimal PE32 at 0x0 with .rsrc at RVA 0x2000, file offset 0x200
        let rsrc_rva: u32 = 0x2000;
        let rsrc_file_off: usize = 0x200;

        // Build .rsrc section bytes:
        // Root dir @ 0 (16 + 16 = 32 bytes): ID 3 -> 0x20, ID 14 -> 0x40
        // Type 3 (RT_ICON) name/id dir @ 0x20: count = icon_payloads.len()
        // For simplicity lay out dynamically.
        let mut rsrc = Vec::new();
        let root_len = 16 + 2 * 8; // 32
        rsrc.resize(root_len, 0);

        let write_u16 = |buf: &mut Vec<u8>, off: usize, v: u16| {
            buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        let write_u32 = |buf: &mut Vec<u8>, off: usize, v: u32| {
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };

        // Root dir: 0 named, 2 id entries
        write_u16(&mut rsrc, 14, 2);

        // RT_ICON dir (type 3)
        let rt_icon_dir_off = rsrc.len();
        write_u32(&mut rsrc, 16, 3);
        write_u32(&mut rsrc, 20, 0x8000_0000 | rt_icon_dir_off as u32);
        rsrc.resize(rt_icon_dir_off + 16 + icon_payloads.len() * 8, 0);
        write_u16(&mut rsrc, rt_icon_dir_off + 14, icon_payloads.len() as u16);

        // RT_GROUP_ICON dir (type 14) with 1 group id=1
        let rt_grp_dir_off = rsrc.len();
        write_u32(&mut rsrc, 24, 14);
        write_u32(&mut rsrc, 28, 0x8000_0000 | rt_grp_dir_off as u32);
        rsrc.resize(rt_grp_dir_off + 16 + 8, 0);
        write_u16(&mut rsrc, rt_grp_dir_off + 14, 1);

        // Group lang dir + data entry
        let grp_lang_off = rsrc.len();
        write_u32(&mut rsrc, rt_grp_dir_off + 16, 1);
        write_u32(&mut rsrc, rt_grp_dir_off + 20, 0x8000_0000 | grp_lang_off as u32);
        rsrc.resize(grp_lang_off + 16 + 8, 0);
        write_u16(&mut rsrc, grp_lang_off + 14, 1);

        let grp_data_entry_off = rsrc.len();
        write_u32(&mut rsrc, grp_lang_off + 16, 1033);
        write_u32(&mut rsrc, grp_lang_off + 20, grp_data_entry_off as u32);
        rsrc.resize(grp_data_entry_off + 16, 0);

        // Icon lang dirs + data entries
        let mut icon_data_entry_offs = Vec::new();
        for (idx, (id, _)) in icon_payloads.iter().enumerate() {
            let lang_off = rsrc.len();
            write_u32(&mut rsrc, rt_icon_dir_off + 16 + idx * 8, *id as u32);
            write_u32(&mut rsrc, rt_icon_dir_off + 16 + idx * 8 + 4, 0x8000_0000 | lang_off as u32);
            rsrc.resize(lang_off + 16 + 8, 0);
            write_u16(&mut rsrc, lang_off + 14, 1);

            let de_off = rsrc.len();
            write_u32(&mut rsrc, lang_off + 16, 1033);
            write_u32(&mut rsrc, lang_off + 20, de_off as u32);
            rsrc.resize(de_off + 16, 0);
            icon_data_entry_offs.push(de_off);
        }

        // Write GRPICONDIR bytes
        let grp_blob_off = rsrc.len();
        let mut grp_blob = Vec::new();
        grp_blob.extend_from_slice(&0u16.to_le_bytes());
        grp_blob.extend_from_slice(&1u16.to_le_bytes());
        grp_blob.extend_from_slice(&(grp_entries.len() as u16).to_le_bytes());
        for &(w, h, bpp, id) in grp_entries {
            let sz = icon_payloads
                .iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, d)| d.len() as u32)
                .unwrap_or(0);
            grp_blob.push(w);
            grp_blob.push(h);
            grp_blob.push(0);
            grp_blob.push(0);
            grp_blob.extend_from_slice(&1u16.to_le_bytes());
            grp_blob.extend_from_slice(&bpp.to_le_bytes());
            grp_blob.extend_from_slice(&sz.to_le_bytes());
            grp_blob.extend_from_slice(&id.to_le_bytes());
        }
        rsrc.extend_from_slice(&grp_blob);
        write_u32(&mut rsrc, grp_data_entry_off, rsrc_rva + grp_blob_off as u32);
        write_u32(&mut rsrc, grp_data_entry_off + 4, grp_blob.len() as u32);

        // Write icon payloads
        for (idx, (_, payload)) in icon_payloads.iter().enumerate() {
            let off = rsrc.len();
            rsrc.extend_from_slice(payload);
            let de_off = icon_data_entry_offs[idx];
            write_u32(&mut rsrc, de_off, rsrc_rva + off as u32);
            write_u32(&mut rsrc, de_off + 4, payload.len() as u32);
        }

        let rsrc_len = rsrc.len() as u32;
        let mut pe = vec![0u8; rsrc_file_off + rsrc.len()];
        pe[0..2].copy_from_slice(b"MZ");
        let e_lfanew: usize = 0x40;
        pe[60..64].copy_from_slice(&(e_lfanew as u32).to_le_bytes());

        pe[e_lfanew..e_lfanew + 4].copy_from_slice(b"PE\0\0");
        pe[e_lfanew + 4..e_lfanew + 6].copy_from_slice(&0x014cu16.to_le_bytes()); // x86
        pe[e_lfanew + 6..e_lfanew + 8].copy_from_slice(&1u16.to_le_bytes()); // 1 section
        let opt_size: u16 = 224; // standard PE32
        pe[e_lfanew + 20..e_lfanew + 22].copy_from_slice(&opt_size.to_le_bytes());

        let opt_off = e_lfanew + 24;
        pe[opt_off..opt_off + 2].copy_from_slice(&0x010bu16.to_le_bytes()); // PE32
        pe[opt_off + 92..opt_off + 96].copy_from_slice(&16u32.to_le_bytes()); // NumberOfRvaAndSizes
        // DataDirectory[2] (.rsrc) at opt_off + 96 + 16
        pe[opt_off + 96 + 16..opt_off + 96 + 20].copy_from_slice(&rsrc_rva.to_le_bytes());
        pe[opt_off + 96 + 20..opt_off + 96 + 24].copy_from_slice(&rsrc_len.to_le_bytes());

        let sec_off = opt_off + opt_size as usize;
        pe[sec_off..sec_off + 5].copy_from_slice(b".rsrc");
        pe[sec_off + 8..sec_off + 12].copy_from_slice(&rsrc_len.to_le_bytes());
        pe[sec_off + 12..sec_off + 16].copy_from_slice(&rsrc_rva.to_le_bytes());
        pe[sec_off + 16..sec_off + 20].copy_from_slice(&rsrc_len.to_le_bytes());
        pe[sec_off + 20..sec_off + 24].copy_from_slice(&(rsrc_file_off as u32).to_le_bytes());

        pe[rsrc_file_off..rsrc_file_off + rsrc.len()].copy_from_slice(&rsrc);
        pe
    }

    #[test]
    fn extracts_highest_res_png_icon_from_pe_rsrc() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("app.exe");
        let small = make_rgba_png(16, 16);
        let large = make_rgba_png(64, 64);

        let pe = build_synthetic_pe_with_rsrc(
            &[(1, small), (2, large)],
            &[(16, 16, 32, 1), (64, 64, 32, 2)],
        );
        fs::write(&exe, pe).unwrap();

        let extracted = extract_best_icon_png_from_pe(&exe).unwrap();
        let img = image::load_from_memory(&extracted).unwrap();
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 64);
    }

    #[test]
    fn picks_largest_frame_from_multi_entry_ico() {
        let p16 = make_rgba_png(16, 16);
        let p48 = make_rgba_png(48, 48);

        // Build 2-entry ICO with PNG payloads
        let mut ico = Vec::new();
        ico.extend_from_slice(&[0, 0, 1, 0, 2, 0]);
        let off1 = 6u32 + 32;
        let off2 = off1 + p16.len() as u32;

        // Entry 1 (16x16)
        ico.extend_from_slice(&[16, 16, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(p16.len() as u32).to_le_bytes());
        ico.extend_from_slice(&off1.to_le_bytes());

        // Entry 2 (48x48)
        ico.extend_from_slice(&[48, 48, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(p48.len() as u32).to_le_bytes());
        ico.extend_from_slice(&off2.to_le_bytes());

        ico.extend_from_slice(&p16);
        ico.extend_from_slice(&p48);

        let out = decode_best_ico_to_png(&ico).unwrap();
        let decoded = image::load_from_memory(&out).unwrap();
        assert_eq!(decoded.width(), 48);
        assert_eq!(decoded.height(), 48);
    }

    #[test]
    fn sync_game_icon_handles_exe_override_swap_and_missing_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("cache");
        let exe = tmp.path().join("game.exe");
        let pe = build_synthetic_pe_with_rsrc(&[(1, make_rgba_png(32, 32))], &[(32, 32, 32, 1)]);
        fs::write(&exe, pe).unwrap();

        // Inherits from .exe
        let res = sync_game_icon_in(&cache, "pfx-game", &exe, None, false).unwrap();
        assert_eq!(res, Some(cache.join("pfx-game.png")));
        assert!(cache.join("pfx-game.png").is_file());

        // Swap to .svg override -> removes old .png cleanly
        let svg_ov = tmp.path().join("custom.svg");
        fs::write(&svg_ov, b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'></svg>").unwrap();
        let res2 = sync_game_icon_in(&cache, "pfx-game", &exe, Some(&svg_ov), true).unwrap();
        assert_eq!(res2, Some(cache.join("pfx-game.svg")));
        assert!(cache.join("pfx-game.svg").is_file());
        assert!(!cache.join("pfx-game.png").exists());

        // Revert back to .exe -> removes .svg and extracts .png
        let res3 = sync_game_icon_in(&cache, "pfx-game", &exe, None, true).unwrap();
        assert_eq!(res3, Some(cache.join("pfx-game.png")));
        assert!(!cache.join("pfx-game.svg").exists());

        // Missing or non-PE exe cleans up and returns Ok(None)
        let bad = tmp.path().join("not_pe.bat");
        fs::write(&bad, b"echo hi").unwrap();
        let res4 = sync_game_icon_in(&cache, "pfx-game", &bad, None, true).unwrap();
        assert_eq!(res4, None);
        assert!(!cache.join("pfx-game.png").exists());
    }
}
