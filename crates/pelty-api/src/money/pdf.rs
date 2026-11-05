//! Server-side invoice PDF (A4, embedded subset DejaVu Sans, full Unicode text incl. Romanian diacritics). Pure
//! layout from an already-localized [`InvoiceDoc`]; no I/O.
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use std::cell::RefCell;
use std::collections::BTreeMap;
use subsetter::GlyphRemapper;

pub struct Line {
    pub description: String,
    pub quantity: String,
    pub unit_price: String,
    pub tax: String,
    pub amount: String,
}

pub struct InvoiceDoc {
    pub title: String,
    pub number: String,
    /// (label, value) pairs under the title (dates, status …).
    pub meta: Vec<(String, String)>,
    pub seller: Vec<String>,
    pub bill_to_label: String,
    pub bill_to: Vec<String>,
    /// Column headers: description, qty, unit price, tax, amount.
    pub headers: [String; 5],
    pub lines: Vec<Line>,
    /// (label, value, bold) rows under the table.
    pub totals: Vec<(String, String, bool)>,
    pub notes: Vec<String>,
    pub footer: Vec<String>,
    pub accent: Option<(f32, f32, f32)>,
    pub page_label: String,
    /// Company logo drawn top-left above the seller block.
    pub logo: Option<Logo>,
}

/// Decoded logo: 8-bit RGB pixels plus an optional alpha mask.
pub struct Logo {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

impl Logo {
    /// Decode PNG/JPEG/WebP bytes, downscaled to at most 256 px.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 2_000_000 {
            return None;
        }
        let img = image::load_from_memory(bytes).ok()?;
        let img = if img.width() > 256 || img.height() > 256 { img.thumbnail(256, 256) } else { img };
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        if width == 0 || height == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for px in rgba.pixels() {
            rgb.extend_from_slice(&px.0[..3]);
            alpha.push(px.0[3]);
        }
        let opaque = alpha.iter().all(|a| *a == 255);
        Some(Self { width, height, rgb, alpha: (!opaque).then_some(alpha) })
    }
}

const W: f32 = 595.0;
const H: f32 = 842.0;
const M: f32 = 48.0;
const REG: Name<'static> = Name(b"F1");
const BOLD: Name<'static> = Name(b"F2");
const IMG: Name<'static> = Name(b"Im1");
const LOGO_H: f32 = 36.0;

static DEJAVU: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans.ttf");
static DEJAVU_BOLD: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf");

/// An embedded TrueType font (DejaVu Sans, Bitstream Vera licence) that
/// records the glyphs used so only those are subset into the file.
struct Font {
    face: ttf_parser::Face<'static>,
    data: &'static [u8],
    name: &'static [u8],
    map: RefCell<GlyphRemapper>,
    /// new gid -> (old gid, char)
    used: RefCell<BTreeMap<u16, (u16, char)>>,
}

impl Font {
    fn new(data: &'static [u8], name: &'static [u8]) -> Self {
        // The bundled fonts are fixed assets; parsing them cannot fail.
        let face = ttf_parser::Face::parse(data, 0).expect("bundled font");
        Self { face, data, name, map: RefCell::new(GlyphRemapper::new()), used: RefCell::new(BTreeMap::new()) }
    }
    fn gid(&self, c: char) -> u16 {
        let c = if c == '\t' { ' ' } else { c };
        self.face.glyph_index(c).or_else(|| self.face.glyph_index('?')).map_or(0, |g| g.0)
    }
    fn upem(&self) -> f32 {
        f32::from(self.face.units_per_em())
    }
    fn advance(&self, gid: u16) -> f32 {
        f32::from(self.face.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap_or(0)) * 1000.0 / self.upem()
    }
    fn width(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.advance(self.gid(c))).sum::<f32>() * size / 1000.0
    }
    fn encode(&self, s: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(s.len() * 2);
        for c in s.chars() {
            let old = self.gid(c);
            let new = self.map.borrow_mut().remap(old);
            self.used.borrow_mut().entry(new).or_insert((old, if c == '\t' { ' ' } else { c }));
            out.extend_from_slice(&new.to_be_bytes());
        }
        out
    }
}

struct Fonts {
    reg: Font,
    bold: Font,
}

impl Fonts {
    fn new() -> Self {
        Self { reg: Font::new(DEJAVU, b"PELTYA+DejaVuSans"), bold: Font::new(DEJAVU_BOLD, b"PELTYB+DejaVuSans-Bold") }
    }
    fn get(&self, bold: bool) -> &Font {
        if bold { &self.bold } else { &self.reg }
    }
    fn width(&self, s: &str, size: f32, bold: bool) -> f32 {
        self.get(bold).width(s, size)
    }
    /// Cut `s` so it fits in `max` points, adding "…".
    fn fit(&self, s: &str, size: f32, max: f32) -> String {
        if self.width(s, size, false) <= max {
            return s.to_string();
        }
        let mut t: String = s.to_string();
        let ell = self.width("…", size, false);
        while !t.is_empty() && self.width(&t, size, false) + ell > max {
            t.pop();
        }
        t.push('…');
        t
    }
}

struct Page {
    c: Content,
}

impl Page {
    fn text(&mut self, f: &Fonts, x: f32, y: f32, size: f32, bold: bool, s: &str) {
        let bytes = f.get(bold).encode(s);
        self.c.begin_text();
        self.c.set_font(if bold { BOLD } else { REG }, size);
        self.c.next_line(x, y);
        self.c.show(Str(&bytes));
        self.c.end_text();
    }
    fn right(&mut self, f: &Fonts, x: f32, y: f32, size: f32, bold: bool, s: &str) {
        let w = f.width(s, size, bold);
        self.text(f, x - w, y, size, bold, s);
    }
    fn rule(&mut self, y: f32, gray: f32) {
        self.c.set_fill_rgb(gray, gray, gray);
        self.c.rect(M, y, W - 2.0 * M, 0.6);
        self.c.fill_nonzero();
        self.c.set_fill_rgb(0.0, 0.0, 0.0);
    }
}

// Column right edges (description is left-aligned at M).
const COLS: [f32; 4] = [355.0, 430.0, 480.0, W - M];

pub fn render(d: &InvoiceDoc) -> Vec<u8> {
    let fx = Fonts::new();
    let mut pages: Vec<Page> = Vec::new();
    let mut p = Page { c: Content::new() };
    let accent = d.accent.unwrap_or((0.15, 0.15, 0.15));

    // Header: accent bar, seller block left, title + meta right.
    p.c.set_fill_rgb(accent.0, accent.1, accent.2);
    p.c.rect(0.0, H - 8.0, W, 8.0);
    p.c.fill_nonzero();
    p.c.set_fill_rgb(0.0, 0.0, 0.0);
    let mut y = H - M - 10.0;
    if let Some(l) = &d.logo {
        let w = (LOGO_H * l.width as f32 / l.height as f32).min(200.0);
        p.c.save_state();
        p.c.transform([w, 0.0, 0.0, LOGO_H, M, H - M - LOGO_H + 4.0]);
        p.c.x_object(IMG);
        p.c.restore_state();
        y -= LOGO_H + 4.0;
    }
    for (i, l) in d.seller.iter().enumerate() {
        p.text(&fx, M, y, if i == 0 { 14.0 } else { 9.0 }, i == 0, &fx.fit(l, 9.0, 260.0));
        y -= if i == 0 { 18.0 } else { 12.0 };
    }
    let mut ry = H - M - 10.0;
    p.c.set_fill_rgb(accent.0, accent.1, accent.2);
    p.right(&fx, W - M, ry, 20.0, true, &d.title);
    p.c.set_fill_rgb(0.0, 0.0, 0.0);
    ry -= 18.0;
    p.right(&fx, W - M, ry, 11.0, true, &d.number);
    ry -= 16.0;
    for (k, v) in &d.meta {
        p.right(&fx, W - M - 90.0, ry, 9.0, false, k);
        p.right(&fx, W - M, ry, 9.0, true, v);
        ry -= 12.0;
    }
    y = y.min(ry) - 16.0;
    p.text(&fx, M, y, 8.0, true, &d.bill_to_label.to_uppercase());
    y -= 13.0;
    for l in &d.bill_to {
        p.text(&fx, M, y, 10.0, false, &fx.fit(l, 10.0, 300.0));
        y -= 13.0;
    }
    y -= 14.0;

    let header = |p: &mut Page, y: f32| {
        p.c.set_fill_rgb(0.95, 0.95, 0.95);
        p.c.rect(M, y - 5.0, W - 2.0 * M, 18.0);
        p.c.fill_nonzero();
        p.c.set_fill_rgb(0.0, 0.0, 0.0);
        p.text(&fx, M + 4.0, y, 8.5, true, &d.headers[0]);
        for (i, h) in d.headers[1..].iter().enumerate() {
            p.right(&fx, COLS[i] - 4.0, y, 8.5, true, h);
        }
    };
    header(&mut p, y);
    y -= 20.0;
    let bottom = M + 40.0;
    for l in &d.lines {
        if y < bottom {
            pages.push(std::mem::replace(&mut p, Page { c: Content::new() }));
            y = H - M - 10.0;
            header(&mut p, y);
            y -= 20.0;
        }
        p.text(&fx, M + 4.0, y, 9.0, false, &fx.fit(&l.description, 9.0, COLS[0] - M - 80.0));
        for (i, v) in [&l.quantity, &l.unit_price, &l.tax, &l.amount].iter().enumerate() {
            p.right(&fx, COLS[i] - 4.0, y, 9.0, false, v);
        }
        y -= 6.0;
        p.rule(y, 0.88);
        y -= 12.0;
    }

    // Totals + notes need ~ (rows + notes) * 14 points.
    let need = (d.totals.len() + d.notes.len()) as f32 * 14.0 + 20.0;
    if y - need < bottom {
        pages.push(std::mem::replace(&mut p, Page { c: Content::new() }));
        y = H - M - 10.0;
    }
    y -= 4.0;
    for (k, v, bold) in &d.totals {
        let size = if *bold { 11.0 } else { 9.5 };
        p.right(&fx, COLS[2] - 4.0, y, size, *bold, k);
        p.right(&fx, COLS[3] - 4.0, y, size, *bold, v);
        y -= if *bold { 17.0 } else { 14.0 };
    }
    y -= 10.0;
    for n in &d.notes {
        p.text(&fx, M, y, 8.5, false, &fx.fit(n, 8.5, W - 2.0 * M));
        y -= 12.0;
    }
    pages.push(p);

    // Footer + page numbers, then assemble.
    let total = pages.len();
    let mut pdf = Pdf::new();
    let catalog = Ref::new(1);
    let tree = Ref::new(2);
    let (f1, f2) = (Ref::new(3), Ref::new(4));
    let page_ids: Vec<Ref> = (0..total).map(|i| Ref::new(5 + 2 * i as i32)).collect();
    let mut next = 5 + 2 * total as i32;
    let img = Ref::new(next);
    let smask = Ref::new(next + 1);
    next += 2;
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids(page_ids.iter().copied()).count(total as i32);
    for (i, mut pg) in pages.into_iter().enumerate() {
        pg.rule(M + 22.0, 0.8);
        let mut fy = M + 10.0;
        for l in d.footer.iter().take(2) {
            pg.text(&fx, M, fy, 7.5, false, &fx.fit(l, 7.5, W - 2.0 * M - 60.0));
            fy -= 10.0;
        }
        let label = format!("{} {}/{}", d.page_label, i + 1, total);
        pg.right(&fx, W - M, M + 10.0, 7.5, false, &label);
        let id = page_ids[i];
        let content = Ref::new(id.get() + 1);
        let mut page = pdf.page(id);
        page.media_box(Rect::new(0.0, 0.0, W, H)).parent(tree).contents(content);
        {
            let mut res = page.resources();
            res.fonts().pair(REG, f1).pair(BOLD, f2);
            if d.logo.is_some() && i == 0 {
                res.x_objects().pair(IMG, img);
            }
        }
        page.finish();
        pdf.stream(content, &pg.c.finish());
    }
    if let Some(l) = &d.logo {
        let mut x = pdf.image_xobject(img, &l.rgb);
        x.width(l.width as i32).height(l.height as i32).color_space().device_rgb();
        x.bits_per_component(8);
        if l.alpha.is_some() {
            x.s_mask(smask);
        }
        x.finish();
        if let Some(a) = &l.alpha {
            let mut m = pdf.image_xobject(smask, a);
            m.width(l.width as i32).height(l.height as i32).color_space().device_gray();
            m.bits_per_component(8);
            m.finish();
        }
    }
    write_font(&mut pdf, f1, &fx.reg, &mut next);
    write_font(&mut pdf, f2, &fx.bold, &mut next);
    pdf.set_file_id((d.number.as_bytes().to_vec(), d.number.as_bytes().to_vec()));
    pdf.finish()
}

/// Type0/Identity-H font with a subset TrueType program, widths and a
/// ToUnicode map so text stays searchable and copyable.
fn write_font(pdf: &mut Pdf, id: Ref, f: &Font, next: &mut i32) {
    let mut alloc = || {
        let r = Ref::new(*next);
        *next += 1;
        r
    };
    let (cid, desc, file, cmap) = (alloc(), alloc(), alloc(), alloc());
    let name = Name(f.name);
    pdf.type0_font(id).base_font(name).encoding_predefined(Name(b"Identity-H")).descendant_font(cid).to_unicode(cmap);
    let used = f.used.borrow();
    let widths: Vec<f32> = (0..f.map.borrow().num_gids()).map(|n| used.get(&n).map_or(0.0, |(old, _)| f.advance(*old))).collect();
    let sys = SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };
    let mut c = pdf.cid_font(cid);
    c.subtype(CidFontType::Type2).base_font(name).system_info(sys).font_descriptor(desc).default_width(0.0);
    c.cid_to_gid_map_predefined(Name(b"Identity"));
    c.widths().consecutive(0, widths);
    c.finish();
    let u = f.upem();
    let s = |v: i16| f32::from(v) * 1000.0 / u;
    let bb = f.face.global_bounding_box();
    pdf.font_descriptor(desc)
        .name(name)
        .flags(FontFlags::NON_SYMBOLIC)
        .bbox(Rect::new(s(bb.x_min), s(bb.y_min), s(bb.x_max), s(bb.y_max)))
        .italic_angle(0.0)
        .ascent(s(f.face.ascender()))
        .descent(s(f.face.descender()))
        .cap_height(s(f.face.capital_height().unwrap_or(f.face.ascender())))
        .stem_v(80.0)
        .font_file2(file);
    // Subsetting the bundled font only fails on a library bug; fall back to
    // the full program so the PDF still renders.
    let program = subsetter::subset(f.data, 0, &f.map.borrow()).unwrap_or_else(|_| f.data.to_vec());
    let len = program.len() as i32;
    pdf.stream(file, &program).pair(Name(b"Length1"), len);
    let mut cm = UnicodeCmap::new(Name(b"Pelty-UCS"), sys);
    for (n, (_, ch)) in used.iter() {
        cm.pair(*n, *ch);
    }
    pdf.cmap(cmap, &cm.finish());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(n: usize) -> InvoiceDoc {
        InvoiceDoc {
            title: "Factură".into(),
            number: "INV-000042".into(),
            meta: vec![("Data".into(), "2026-10-29".into())],
            seller: vec!["Pelty SRL".into(), "Strada Ștefan 1".into()],
            bill_to_label: "Bill to".into(),
            bill_to: vec!["Alice".into()],
            headers: ["Description".into(), "Qty".into(), "Unit".into(), "Tax".into(), "Amount".into()],
            lines: (0..n).map(|i| Line { description: format!("Line {i} {}", "x".repeat(200)), quantity: "1".into(), unit_price: "10.00".into(), tax: "19%".into(), amount: "10.00 €".into() }).collect(),
            totals: vec![("Total".into(), "11.90 EUR".into(), true)],
            notes: vec!["Reverse charge".into()],
            footer: vec!["IBAN RO00".into()],
            accent: Some((0.1, 0.2, 0.7)),
            page_label: "Page".into(),
            logo: None,
        }
    }

    fn pages(b: &[u8]) -> usize {
        b.windows(b"/Type /Page\n".len()).filter(|w| *w == b"/Type /Page\n").count()
            + b.windows(b"/Type /Page/".len()).filter(|w| *w == b"/Type /Page/").count()
            + b.windows(b"/Type /Page ".len()).filter(|w| *w == b"/Type /Page ").count()
    }

    #[test]
    fn renders_valid_pdf_and_paginates() {
        let one = render(&doc(3));
        assert!(one.starts_with(b"%PDF-"));
        assert!(one.ends_with(b"%%EOF") || one.ends_with(b"%%EOF\n"));
        assert!(one.windows(10).any(|w| w == b"INV-000042"));
        let many = render(&doc(120));
        assert!(pages(&many) > pages(&one), "{} vs {}", pages(&many), pages(&one));
    }

    #[test]
    fn embeds_subset_font_with_unicode_map() {
        let mut d = doc(3);
        d.title = "Factură ȘȚĂÎÂ șțăîâ €".into();
        let b = render(&d);
        let t = String::from_utf8_lossy(&b);
        assert!(t.contains("/FontFile2") && t.contains("/Identity-H") && t.contains("/ToUnicode"));
        assert!(!t.contains("Helvetica"));
        // Subset: far smaller than the two bundled fonts (~1.4 MB).
        assert!(b.len() < 200_000, "{}", b.len());
        // ToUnicode maps ș (U+0219) and ț (U+021B).
        assert!(t.contains("<0219>") && t.contains("<021B>"));
    }

    #[test]
    fn embeds_logo_with_alpha() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_fn(400, 100, |x, _| image::Rgba([89, 181, 211, if x < 200 { 255 } else { 0 }]));
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let logo = Logo::decode(&png).expect("decodes");
        assert_eq!((logo.width, logo.height), (256, 64), "downscaled, aspect kept");
        assert!(logo.alpha.is_some());
        let mut d = doc(2);
        d.logo = Some(logo);
        let b = render(&d);
        let t = String::from_utf8_lossy(&b);
        assert!(t.contains("/Subtype /Image") && t.contains("/SMask") && t.contains("/Im1"));
        assert!(Logo::decode(b"not an image").is_none());
    }

    #[test]
    fn long_text_is_cut() {
        let fx = Fonts::new();
        let s = fx.fit(&"W".repeat(500), 9.0, 100.0);
        assert!(fx.width(&s, 9.0, false) <= 100.0 && s.ends_with('…'));
        assert_eq!(fx.fit("short", 9.0, 100.0), "short");
    }
}
