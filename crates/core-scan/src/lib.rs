//! Reading a printed card from a photo (DECISIONS.md, "Lettura di schede da
//! foto"). Every card has the same layout, so the work is deterministic:
//!
//! 1. find the card, the largest light, colourless area, and its corners;
//! 2. straighten it to the layout's size, trying the orientations in which a
//!    QR box decodes;
//! 3. read each box: text through Tesseract (when installed), QR codes,
//!    pictures as JPEG crops.

use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use core_model::config::{ScanBox, ScanDef, ScanRead};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::{self, FilterType};
use image::{DynamicImage, GrayImage, ImageDecoder, ImageReader, Luma, Rgb, RgbImage};
use imageproc::contrast::{ThresholdType, otsu_level, threshold};
use imageproc::geometric_transformations::{Interpolation, Projection, warp_into};
use imageproc::region_labelling::{Connectivity, connected_components};

/// The card is searched on a copy this wide.
const SEARCH_WIDTH: u32 = 1000;
/// Saturation above which a pixel is not part of a white card.
const MAX_SATURATION: i32 = 60;
/// A card is light even in a dim photo.
const MIN_LIGHTNESS: i32 = 100;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("the photo cannot be read: {0}")]
    Decode(String),
    #[error("no card found in the photo")]
    NoCard,
    #[error("cannot encode the picture: {0}")]
    Encode(String),
}

/// What was read, by field id.
#[derive(Debug, Default, Clone)]
pub struct Scan {
    pub texts: BTreeMap<String, String>,
    /// QR contents.
    pub codes: BTreeMap<String, String>,
    /// JPEG crops.
    pub images: BTreeMap<String, Vec<u8>>,
    /// Whether text recognition was available.
    pub ocr: bool,
}

/// Text recognition of one box, already cleaned up.
pub trait Ocr {
    fn read(&self, img: &GrayImage, read: ScanRead, language: Option<&str>) -> Option<String>;
}

/// The Tesseract command line, when installed.
#[derive(Debug, Clone)]
pub struct Tesseract {
    bin: PathBuf,
}

impl Tesseract {
    /// Looks in `PATH` and in the usual install places (an app started
    /// from the desktop does not see the shell's `PATH`).
    pub fn find() -> Option<Self> {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
        dirs.into_iter()
            .map(|d| d.join("tesseract"))
            .find(|p| p.is_file())
            .map(|bin| Self { bin })
    }

    fn run(&self, png: &[u8], args: &[&str]) -> Option<String> {
        let mut child = Command::new(&self.bin)
            .args(["stdin", "stdout"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        child.stdin.take()?.write_all(png).ok()?;
        let out = child.wait_with_output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl Ocr for Tesseract {
    fn read(&self, img: &GrayImage, read: ScanRead, language: Option<&str>) -> Option<String> {
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .ok()?;
        let mut args = vec!["--psm", if read == ScanRead::Text { "6" } else { "7" }];
        if read == ScanRead::Digits {
            args.extend(["-c", "tessedit_char_whitelist=0123456789"]);
        }
        // A language that is not installed fails: fall back to the default.
        let text = match language {
            Some(l) => {
                let mut with = args.clone();
                with.extend(["-l", l]);
                self.run(&png, &with).or_else(|| self.run(&png, &args))
            }
            None => self.run(&png, &args),
        }?;
        let text = tidy(&text);
        (!text.is_empty()).then_some(text)
    }
}

/// Trims dotted leaders, stray punctuation and blank lines.
fn tidy(text: &str) -> String {
    let junk: &[char] = &['.', '_', '…', '·', ' ', '|', ',', ':', ';'];
    text.lines()
        .map(|l| {
            let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
            let l = l.trim_start_matches(junk);
            // Keep a closing parenthesis or digit, drop trailing leaders.
            l.trim_end_matches(junk).to_string()
        })
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decodes a photo, turned upright as its EXIF orientation says.
fn decode(bytes: &[u8]) -> Result<RgbImage, ScanError> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ScanError::Decode(e.to_string()))?;
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| ScanError::Decode(e.to_string()))?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| ScanError::Decode(e.to_string()))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img.to_rgb8())
}

/// The card's corners (top-left, top-right, bottom-right, bottom-left) in
/// `img`: the extreme points along the diagonals of the largest light,
/// colourless connected area.
fn card_corners(img: &RgbImage) -> Option<[(f32, f32); 4]> {
    let gray = imageops::grayscale(img);
    let level = i32::from(otsu_level(&gray));
    let mask = GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let (r, g, b) = (i32::from(p[0]), i32::from(p[1]), i32::from(p[2]));
        let l = (r + g + b) / 3;
        let light = l > level && l > MIN_LIGHTNESS;
        let sat = r.max(g).max(b) - r.min(g).min(b);
        Luma([if light && sat < MAX_SATURATION { 255 } else { 0 }])
    });
    let labels = connected_components(&mask, Connectivity::Four, Luma([0u8]));
    let mut sizes: BTreeMap<u32, usize> = BTreeMap::new();
    for l in labels.pixels() {
        if l[0] != 0 {
            *sizes.entry(l[0]).or_default() += 1;
        }
    }
    let (&best, &size) = sizes.iter().max_by_key(|(_, n)| **n)?;
    // A card fills a good part of the photo.
    if size < (img.width() * img.height()) as usize / 10 {
        return None;
    }
    let mut c = [(0f32, 0f32); 4];
    let mut score = [f32::MAX, f32::MIN, f32::MIN, f32::MAX];
    for (x, y, l) in labels.enumerate_pixels() {
        if l[0] != best {
            continue;
        }
        let (fx, fy) = (x as f32, y as f32);
        let (sum, diff) = (fx + fy, fx - fy);
        if sum < score[0] {
            (score[0], c[0]) = (sum, (fx, fy));
        }
        if diff > score[1] {
            (score[1], c[1]) = (diff, (fx, fy));
        }
        if sum > score[2] {
            (score[2], c[2]) = (sum, (fx, fy));
        }
        if diff < score[3] {
            (score[3], c[3]) = (diff, (fx, fy));
        }
    }
    Some(c)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Straightens the quadrilateral `c` to `w` × `h`.
fn straighten(img: &RgbImage, c: [(f32, f32); 4], w: u32, h: u32) -> Option<RgbImage> {
    let (fw, fh) = (w as f32, h as f32);
    let to = [(0.0, 0.0), (fw, 0.0), (fw, fh), (0.0, fh)];
    let p = Projection::from_control_points(c, to)?;
    let mut out = RgbImage::new(w, h);
    warp_into(img, &p, Interpolation::Bilinear, Rgb([255, 255, 255]), &mut out);
    Some(out)
}

fn crop(img: &RgbImage, at: [u32; 4]) -> RgbImage {
    let [x, y, w, h] = at;
    let x = x.min(img.width().saturating_sub(1));
    let y = y.min(img.height().saturating_sub(1));
    let w = w.min(img.width() - x);
    let h = h.min(img.height() - y);
    imageops::crop_imm(img, x, y, w, h).to_image()
}

/// Decodes a QR code in `img`, trying a few scales and thresholds.
fn read_code(img: &RgbImage) -> Option<String> {
    let gray = imageops::grayscale(img);
    for scale in [1u32, 2, 3] {
        let big = if scale == 1 {
            gray.clone()
        } else {
            imageops::resize(&gray, gray.width() * scale, gray.height() * scale, FilterType::Lanczos3)
        };
        for level in [None, Some(100u8), Some(128), Some(160)] {
            let candidate = match level {
                None => big.clone(),
                Some(t) => threshold(&big, t, ThresholdType::Binary),
            };
            let mut prepared = rqrr::PreparedImage::prepare(candidate);
            for grid in prepared.detect_grids() {
                if let Ok((_, content)) = grid.decode() {
                    return Some(content);
                }
            }
        }
    }
    None
}

/// A box ready for text recognition: enlarged, on plain paper, without
/// what is not writing.
/// - Single lines: dark areas lower than a fifth of the box go (the dots
///   of a leader), unless wide (a dash).
/// - Several lines: small areas far from any letter go (scratches on the
///   sleeve, dust); punctuation sits next to a letter and stays.
fn for_ocr(img: &RgbImage, read: ScanRead) -> GrayImage {
    let g = imageops::grayscale(img);
    // Letters about 40 px tall suit Tesseract best: larger ones make it
    // invent characters.
    let k = if read == ScanRead::Text { 2 } else { 3 };
    let g = imageops::resize(&g, g.width() * k, g.height() * k, FilterType::CatmullRom);
    let b = threshold(&g, otsu_level(&g), ThresholdType::Binary);
    let inverted = GrayImage::from_fn(b.width(), b.height(), |x, y| Luma([255 - b.get_pixel(x, y)[0]]));
    let labels = connected_components(&inverted, Connectivity::Eight, Luma([0u8]));
    let mut boxes: BTreeMap<u32, [u32; 4]> = BTreeMap::new();
    for (x, y, l) in labels.enumerate_pixels() {
        if l[0] == 0 {
            continue;
        }
        let e = boxes.entry(l[0]).or_insert([x, y, x, y]);
        *e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
    }
    let keep: BTreeMap<u32, bool> = if read == ScanRead::Text {
        let small = b.height() / 10;
        let near = b.height() / 6;
        let big: Vec<[u32; 4]> = boxes
            .values()
            .filter(|&&[x0, y0, x1, y1]| x1 - x0 >= small || y1 - y0 >= small)
            .copied()
            .collect();
        boxes
            .iter()
            .map(|(&l, &[x0, y0, x1, y1])| {
                let is_big = x1 - x0 >= small || y1 - y0 >= small;
                let close = big.iter().any(|&[bx0, by0, bx1, by1]| {
                    let dx = bx0.saturating_sub(x1).max(x0.saturating_sub(bx1));
                    let dy = by0.saturating_sub(y1).max(y0.saturating_sub(by1));
                    dx <= near && dy <= near
                });
                (l, is_big || close)
            })
            .collect()
    } else {
        let min_h = b.height() / 5;
        boxes
            .iter()
            .map(|(&l, &[x0, y0, x1, y1])| {
                let (w, h) = (x1 - x0, y1 - y0);
                (l, h >= min_h || (w >= 2 * h && w >= min_h / 2))
            })
            .collect()
    };
    // The paper's shade, for the margin and for what is dropped.
    let (mut sum, mut n) = (0u64, 0u64);
    for (x, y, l) in labels.enumerate_pixels() {
        if l[0] == 0 {
            sum += u64::from(g.get_pixel(x, y)[0]);
            n += 1;
        }
    }
    let paper = Luma([u8::try_from(sum / n.max(1)).unwrap_or(255)]);
    // Only the written part, with a margin: Tesseract invents characters
    // in wide empty space and misreads writing that touches the edge.
    let kept = boxes
        .iter()
        .filter(|(l, _)| keep.get(l).copied().unwrap_or(false))
        .map(|(_, bx)| *bx);
    let Some([x0, y0, x1, y1]) = kept.reduce(|a, c| [a[0].min(c[0]), a[1].min(c[1]), a[2].max(c[2]), a[3].max(c[3])]) else {
        return GrayImage::from_pixel(b.width(), b.height(), paper);
    };
    let m = b.height() / 4;
    GrayImage::from_fn(x1 - x0 + 1 + 2 * m, y1 - y0 + 1 + 2 * m, |x, y| {
        let (Some(bx), Some(by)) = ((x + x0).checked_sub(m), (y + y0).checked_sub(m)) else {
            return paper;
        };
        if bx > x1 || by > y1 {
            return paper;
        }
        // Writing keeps its grey shades, everything else (faint marks
        // too) becomes plain paper.
        let label = labels.get_pixel(bx, by)[0];
        let k = label != 0 && keep.get(&label).copied().unwrap_or(false);
        if k { *g.get_pixel(bx, by) } else { paper }
    })
}

fn jpeg(img: &RgbImage) -> Result<Vec<u8>, ScanError> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(img)
        .map_err(|e| ScanError::Encode(e.to_string()))?;
    Ok(out)
}

/// The card straightened to the layout's size, upright: of the possible
/// orientations, the first in which every QR box decodes.
pub fn straightened(bytes: &[u8], def: &ScanDef) -> Result<RgbImage, ScanError> {
    let img = decode(bytes)?;
    let scale = SEARCH_WIDTH as f32 / img.width().max(1) as f32;
    let small = imageops::resize(
        &img,
        SEARCH_WIDTH,
        ((img.height() as f32 * scale) as u32).max(1),
        FilterType::Triangle,
    );
    let c = card_corners(&small).ok_or(ScanError::NoCard)?;
    let c = c.map(|(x, y)| (x / scale, y / scale));
    // The corners' order follows the photo; the card's long side is its width.
    let landscape = def.width >= def.height;
    let photo_landscape = dist(c[0], c[1]) >= dist(c[1], c[2]);
    let turns: [usize; 2] = if landscape == photo_landscape { [0, 2] } else { [1, 3] };
    let codes: Vec<&ScanBox> = def.boxes.iter().filter(|b| b.read == ScanRead::Code).collect();
    let mut first = None;
    for turn in turns {
        let rotated = [c[turn % 4], c[(turn + 1) % 4], c[(turn + 2) % 4], c[(turn + 3) % 4]];
        let Some(card) = straighten(&img, rotated, def.width, def.height) else {
            continue;
        };
        if codes.iter().all(|b| read_code(&crop(&card, b.at)).is_some()) {
            return Ok(card);
        }
        first.get_or_insert(card);
    }
    first.ok_or(ScanError::NoCard)
}

/// Reads every box of `def` from the photo.
pub fn scan(bytes: &[u8], def: &ScanDef, ocr: Option<&dyn Ocr>) -> Result<Scan, ScanError> {
    let card = straightened(bytes, def)?;
    let mut out = Scan {
        ocr: ocr.is_some(),
        ..Scan::default()
    };
    for b in &def.boxes {
        let part = crop(&card, b.at);
        let id = b.field.to_string();
        match b.read {
            ScanRead::Code => {
                if let Some(c) = read_code(&part) {
                    out.codes.insert(id, c);
                }
            }
            ScanRead::Image => {
                out.images.insert(id, jpeg(&part)?);
            }
            read => {
                if let Some(t) = ocr.and_then(|o| o.read(&for_ocr(&part, read), read, def.language.as_deref())) {
                    out.texts.insert(id, t);
                }
            }
        }
    }
    Ok(out)
}

/// The point a map link points at: a route's destination
/// (`/dir/…/lat,lng`), a query (`q=` / `ll=` / `query=`), a place
/// (`!3d…!4d…`) or the view centre (`@lat,lng`), in this order.
pub fn geo_from_link(link: &str) -> Option<(f64, f64)> {
    fn pair(s: &str) -> Option<(f64, f64)> {
        let s = s.trim_start_matches('@');
        let mut it = s.splitn(3, ',');
        let lat: f64 = it.next()?.trim().parse().ok()?;
        let lng: f64 = it
            .next()?
            .trim()
            .trim_end_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .ok()?;
        ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng)).then_some((lat, lng))
    }
    let decoded = link.replace("%2C", ",").replace("%2c", ",");
    let path = decoded.split(['?', '#']).next().unwrap_or_default();
    if let Some(dir) = path.split("/dir/").nth(1) {
        if let Some(p) = dir.split('/').filter(|s| !s.starts_with('@')).filter_map(pair).next_back() {
            return Some(p);
        }
    }
    if let Some(query) = decoded.split('?').nth(1) {
        for kv in query.split('&') {
            if let Some((k, v)) = kv.split_once('=') {
                if matches!(k, "q" | "ll" | "query" | "destination" | "center") {
                    if let Some(p) = pair(v) {
                        return Some(p);
                    }
                }
            }
        }
    }
    if let Some(rest) = decoded.split("!3d").nth(1) {
        if let Some((lat, lng)) = rest.split_once("!4d") {
            if let Some(p) = pair(&format!("{lat},{lng}")) {
                return Some(p);
            }
        }
    }
    path.split('/').find(|s| s.starts_with('@')).and_then(pair)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn map_links() {
        let route = "https://www.google.it/maps/dir//42.3738939,14.1530978/@42.374048,14.1530599,17.5z?entry=ttu";
        assert_eq!(geo_from_link(route), Some((42.3738939, 14.1530978)));
        assert_eq!(
            geo_from_link("https://maps.google.com/?q=45.1,9.2"),
            Some((45.1, 9.2))
        );
        assert_eq!(
            geo_from_link("https://www.google.com/maps/place/X/@41.9,12.4,15z/data=!3d41.90!4d12.49"),
            Some((41.90, 12.49))
        );
        assert_eq!(
            geo_from_link("https://www.google.com/maps/@41.9,12.4,15z"),
            Some((41.9, 12.4))
        );
        assert_eq!(geo_from_link("https://example.com/hello"), None);
        assert_eq!(geo_from_link("https://maps.google.com/?q=95,9"), None);
    }

    /// A white card with a QR in its bottom-right box and a grey band, laid
    /// in perspective on a dark table, `upside_down` if asked.
    fn photo(upside_down: bool) -> Vec<u8> {
        let mut card = RgbImage::from_pixel(600, 400, Rgb([250, 250, 250]));
        for y in 100..200 {
            for x in 50..550 {
                card.put_pixel(x, y, Rgb([120, 120, 120]));
            }
        }
        let code = qrcode::QrCode::new(b"https://maps.google.com/?q=45.1,9.2").unwrap();
        let w = code.width() as u32;
        let module = 120 / (w + 8);
        for (i, c) in code.to_colors().iter().enumerate() {
            if *c == qrcode::Color::Dark {
                let (cx, cy) = (i as u32 % w, i as u32 / w);
                for dy in 0..module {
                    for dx in 0..module {
                        card.put_pixel(460 + (cx + 4) * module + dx - 4 * module, 260 + (cy + 4) * module + dy - 4 * module, Rgb([0, 0, 0]));
                    }
                }
            }
        }
        if upside_down {
            card = imageops::rotate180(&card);
        }
        let mut photo = RgbImage::from_pixel(1200, 900, Rgb([90, 60, 40]));
        let quad = [(200.0, 150.0), (1000.0, 120.0), (1050.0, 760.0), (160.0, 800.0)];
        let p = Projection::from_control_points([(0.0, 0.0), (600.0, 0.0), (600.0, 400.0), (0.0, 400.0)], quad).unwrap();
        warp_into(&card, &p, Interpolation::Bilinear, Rgb([0, 0, 0]), &mut photo);
        // warp_into fills outside the card: put the table back.
        let inv = Projection::from_control_points(quad, [(0.0, 0.0), (600.0, 0.0), (600.0, 400.0), (0.0, 400.0)]).unwrap();
        for (x, y, px) in photo.enumerate_pixels_mut() {
            let (u, v) = inv * (x as f32, y as f32);
            if !(0.0..600.0).contains(&u) || !(0.0..400.0).contains(&v) {
                *px = Rgb([90, 60, 40]);
            }
        }
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(photo).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn layout() -> ScanDef {
        serde_norway::from_str(
            "{ width: 600, height: 400, boxes: [ { field: place, at: [450, 250, 150, 150], read: code }, { field: map, at: [50, 100, 500, 100], read: image } ] }",
        )
        .unwrap()
    }

    #[test]
    fn reads_a_card_in_perspective_either_way_up() {
        for upside_down in [false, true] {
            let s = scan(&photo(upside_down), &layout(), None).unwrap();
            assert_eq!(s.codes.get("place").map(String::as_str), Some("https://maps.google.com/?q=45.1,9.2"), "upside down: {upside_down}");
            let map = image::load_from_memory(&s.images["map"]).unwrap().to_rgb8();
            assert_eq!(map.dimensions(), (500, 100));
            // The band is grey all along: the crop is where it should be.
            let p = map.get_pixel(250, 50);
            assert!(p[0] > 90 && p[0] < 150, "{p:?}");
        }
    }

    #[test]
    fn no_card_no_scan() {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 200, Rgb([90, 60, 40])))
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        assert!(matches!(scan(&out, &layout(), None), Err(ScanError::NoCard)));
    }

    #[test]
    fn tidy_drops_leaders() {
        assert_eq!(tidy("..Chieti - Villaggio...\n\n"), "Chieti - Villaggio");
        assert_eq!(tidy("DA FARE: lati (civili) .\nB"), "DA FARE: lati (civili)\nB");
    }
}
