//! Previews for shelf files: a picture where one can be made, otherwise the
//! opening text of documents and text files. Files with neither get an icon
//! for their type in the frontend.

use std::io::{Read, Seek};
use std::path::Path;

use base64::Engine;
use windows::core::HSTRING;

/// What a shelf tile shows for a file or folder.
#[derive(Clone, serde::Serialize)]
pub struct ShelfPreview {
    /// A PNG preview as a data URI.
    pub thumb: Option<String>,
    /// The opening text of a document or text file, when there's no picture.
    pub text: Option<String>,
    /// The item is a folder (for its placeholder icon).
    pub folder: bool,
}

/// How much opening text a tile shows; it has room for about eight lines.
const TEXT_PREVIEW_CHARS: usize = 400;
/// Largest archive entry read for a preview (an embedded thumbnail or the
/// document body).
const MAX_ENTRY_BYTES: u64 = 8 * 1024 * 1024;

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff",
];
/// Office Open XML and OpenDocument files: zip archives that may carry a
/// thumbnail, and whose text can be read for the word processor and slide
/// formats.
const OFFICE_EXTS: &[&str] = &[
    "docx", "docm", "dotx", "xlsx", "xlsm", "pptx", "pptm", "odt", "ods", "odp",
];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "log", "csv", "tsv", "json", "xml", "yml", "yaml", "toml", "ini",
    "cfg", "conf", "env", "js", "jsx", "ts", "tsx", "mjs", "cjs", "py", "rs", "c", "h", "cpp",
    "hpp", "cs", "java", "kt", "go", "rb", "php", "swift", "html", "htm", "css", "scss", "sh",
    "ps1", "bat", "cmd", "sql", "lua", "srt", "vtt",
];

pub fn preview(path: &str) -> ShelfPreview {
    let p = Path::new(path);
    let folder = p.is_dir();
    if folder {
        return ShelfPreview {
            thumb: shell_thumbnail(path),
            text: None,
            folder,
        };
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let ext = ext.as_str();

    let mut text = None;
    let thumb = if IMAGE_EXTS.contains(&ext) {
        image_thumbnail(p)
    } else if ext == "pdf" {
        pdf_thumbnail(path)
    } else if OFFICE_EXTS.contains(&ext) {
        let embedded = office_thumbnail(p);
        if embedded.is_none() {
            text = office_text(p, ext);
        }
        embedded
    } else {
        None
    };
    // Anything else Windows can thumbnail (videos, fonts, files with a
    // third-party previewer), then the opening lines of text files.
    let thumb = thumb.or_else(|| {
        if text.is_none() {
            shell_thumbnail(path)
        } else {
            None
        }
    });
    if thumb.is_none() && text.is_none() && TEXT_EXTS.contains(&ext) {
        text = plain_text(p);
    }
    ShelfPreview {
        thumb,
        text,
        folder,
    }
}

fn png_data_uri(bytes: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Decodes an image (from a file or memory) into a small PNG preview, with
/// dimension and allocation caps: a small, highly compressed file can still
/// expand into a huge bitmap.
fn decode_thumbnail<R: std::io::BufRead + std::io::Seek>(
    reader: image::ImageReader<R>,
) -> Option<String> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = reader.with_guessed_format().ok()?;
    reader.limits(limits);
    let thumb = reader.decode().ok()?.thumbnail(256, 256);
    let mut png = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png_data_uri(&png))
}

fn image_thumbnail(p: &Path) -> Option<String> {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_file() || meta.len() > 32 * 1024 * 1024 {
        return None;
    }
    decode_thumbnail(image::ImageReader::open(p).ok()?)
}

/// Page one of a PDF, rendered by Windows' built-in PDF engine. Fails for
/// password-protected files, which then show the PDF icon.
fn pdf_thumbnail(path: &str) -> Option<String> {
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};

    com_init();
    let render = || -> windows::core::Result<Vec<u8>> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.get()?;
        let document = PdfDocument::LoadFromFileAsync(&file)?.get()?;
        let page = document.GetPage(0)?;
        let options = PdfPageRenderOptions::new()?;
        // Height follows the page's aspect ratio. The default output is PNG.
        options.SetDestinationWidth(256)?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &options)?
            .get()?;
        let size = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(size)?.get()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes)?;
        Ok(bytes)
    };
    render().ok().map(|png| png_data_uri(&png))
}

fn read_entry<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Option<Vec<u8>> {
    let options = zip::ZipReadOptions::new().enforce_header_consistency(true);
    let entry = archive.by_name_with_options(name, options).ok()?;
    if entry.size() > MAX_ENTRY_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry.take(MAX_ENTRY_BYTES).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

fn open_archive(p: &Path) -> Option<zip::ZipArchive<std::fs::File>> {
    zip::ZipArchive::new(std::fs::File::open(p).ok()?).ok()
}

/// The thumbnail Office or LibreOffice saved inside the file, if any.
fn office_thumbnail(p: &Path) -> Option<String> {
    let mut archive = open_archive(p)?;
    [
        "docProps/thumbnail.jpeg",
        "docProps/thumbnail.jpg",
        "docProps/thumbnail.png",
        "Thumbnails/thumbnail.png",
    ]
    .iter()
    .find_map(|name| {
        let bytes = read_entry(&mut archive, name)?;
        decode_thumbnail(image::ImageReader::new(std::io::Cursor::new(bytes)))
    })
}

/// Opening text of a Word, PowerPoint or OpenDocument text/slide file.
fn office_text(p: &Path, ext: &str) -> Option<String> {
    let entry = match ext {
        "docx" | "docm" | "dotx" => "word/document.xml",
        "pptx" | "pptm" => "ppt/slides/slide1.xml",
        "odt" | "odp" => "content.xml",
        _ => return None,
    };
    let xml = read_entry(&mut open_archive(p)?, entry)?;
    snippet(&xml_text(&String::from_utf8_lossy(&xml)))
}

/// The text content of an XML document, with a line break at the end of each
/// paragraph (Word, PowerPoint and OpenDocument paragraph tags).
fn xml_text(xml: &str) -> String {
    const PARAGRAPH_ENDS: &[&str] = &["</w:p>", "</a:p>", "</text:p>", "</text:h>"];
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        out.push_str(&unescape_xml(&rest[..start]));
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start..start + end + 1];
        if PARAGRAPH_ENDS.contains(&tag) {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
        if out.len() > TEXT_PREVIEW_CHARS * 4 {
            break;
        }
    }
    out
}

fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The first few kilobytes of a text file, unless it looks binary.
fn plain_text(p: &Path) -> Option<String> {
    let mut head = Vec::with_capacity(4096);
    std::fs::File::open(p)
        .ok()?
        .take(4096)
        .read_to_end(&mut head)
        .ok()?;
    if head.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&head);
    snippet(text.trim_start_matches('\u{feff}'))
}

/// Trims text to what a tile can show: blank lines and runs of spaces
/// collapsed, at most TEXT_PREVIEW_CHARS characters.
fn snippet(text: &str) -> Option<String> {
    let lines: Vec<String> = text
        .replace("\r\n", "\n")
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect();
    let joined = lines.join("\n");
    let trimmed: String = joined.chars().take(TEXT_PREVIEW_CHARS).collect();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn com_init() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
}

/// Explorer's thumbnail for a file or folder. Asks for thumbnails only, so a
/// file Windows can't preview returns nothing rather than its generic icon.
fn shell_thumbnail(path: &str) -> Option<String> {
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::UI::Shell::{
        IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF, SIIGBF_BIGGERSIZEOK,
        SIIGBF_THUMBNAILONLY,
    };

    com_init();
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let hbitmap = factory
            .GetImage(
                SIZE { cx: 256, cy: 256 },
                SIIGBF(SIIGBF_THUMBNAILONLY.0 | SIIGBF_BIGGERSIZEOK.0),
            )
            .ok()?;
        if hbitmap.0.is_null() {
            return None;
        }
        let result = crate::utils::hbitmap_to_base64(hbitmap);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(hbitmap.into());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn office_archive() -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(b"<document>Hello</document>").unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_deflated_office_entries() {
        let mut archive = zip::ZipArchive::new(Cursor::new(office_archive())).unwrap();
        assert_eq!(
            read_entry(&mut archive, "word/document.xml").as_deref(),
            Some(b"<document>Hello</document>".as_slice())
        );
        assert!(read_entry(&mut archive, "missing.xml").is_none());
    }

    #[test]
    fn rejects_office_entries_with_inconsistent_header_names() {
        let mut bytes = office_archive();
        // The local filename starts at byte 30; leave the directory name intact.
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        bytes[30] = b'x';
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert!(read_entry(&mut archive, "word/document.xml").is_none());
    }

    #[test]
    fn extracts_paragraph_text() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Hello &amp; welcome</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Second  line</w:t></w:r></w:p></w:body></w:document>"#;
        assert_eq!(
            snippet(&xml_text(xml)).as_deref(),
            Some("Hello & welcome\nSecond line")
        );
    }

    #[test]
    fn trims_snippets() {
        assert_eq!(
            snippet("\n\n  a   b \r\n\r\n c\n").as_deref(),
            Some("a b\nc")
        );
        assert_eq!(snippet("   \n  "), None);
        assert_eq!(
            snippet(&"x".repeat(1000)).map(|s| s.chars().count()),
            Some(TEXT_PREVIEW_CHARS)
        );
    }
}
