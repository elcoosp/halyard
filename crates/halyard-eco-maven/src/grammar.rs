//! The `pom.xml` grammar (M8 ADR-0058), hand-rolled over the VERIFIED
//! reference (`docs/research/maven-pom-grammars.md`, from
//! `maven.apache.org/pom.html` and the `maven-artifact`
//! `ComparableVersion` javadoc):
//!
//! - coordinates: *"groupId:artifactId:version are all required fields
//!   (although, groupId and version do not need to be explicitly defined
//!   if they are inherited from a parent)"* — the scanner reads the
//!   `<project>` children `groupId`, `artifactId`, `version`,
//!   `packaging`, `parent`, `modules`, `dependencies`;
//! - aggregation: *"A pom packaged project may aggregate the build of a
//!   set of projects by listing them as modules, which are relative
//!   paths to the directories or the POM files of those projects"*, and
//!   the parent lookup rides *"...uses the POM it finds there only if
//!   its coordinates match the <parent> element"*;
//! - versions: `ComparableVersion` admits arbitrary strings — the
//!   scanner captures the value verbatim (trimmed, with its byte span);
//!   the semver-canonical decision lives in the adapter, never here.
//!
//! The scanner is offset-preserving like the sibling hand-written
//! lexers (ADR-0001's JSON scanner, ADR-0057's go.mod lexer): spans
//! point into the ORIGINAL bytes, whitespace and comments outside the
//! captured values never move (invariant I4). Comments, the XML
//! declaration, DOCTYPEs, processing instructions and CDATA are
//! consumed; a comment *inside* a captured coordinate value makes that
//! value non-planable (the value's bytes would not survive an edit
//! untouched). Profile blocks are consumed unmodeled with a warning —
//! profile-activated version overrides are build semantics a pure
//! discovery pass does not evaluate (the cargo `[patch]` posture).

use halyard_core::discover::{DiscoverError, ManifestFormat};
use halyard_core::package::Span;

/// One parsed `pom.xml` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pom {
    /// The `<groupId>` of the project itself (`None` = inherited).
    pub group_id: Option<String>,
    /// The `<artifactId>` of the project itself (the maven coordinate's
    /// unique half within one repository).
    pub artifact_id: Option<String>,
    /// The `<version>` of the project itself: the trimmed value text
    /// plus its byte span in the original file (`None` = inherited or
    /// absent).
    pub version: Option<ValueField>,
    /// The `<packaging>` (`jar` is the documented default — the caller
    /// applies it).
    pub packaging: Option<String>,
    /// The `<parent>` block, when present.
    pub parent: Option<ParentRef>,
    /// `<modules>/<module>` entries, document order (relative directory
    /// or pom-file paths).
    pub modules: Vec<String>,
    /// `<dependencies>/<dependency>` rows, document order.
    pub dependencies: Vec<DepRow>,
    /// Non-fatal observations (§6.3): profile blocks consumed unmodeled,
    /// coordinate values a comment corrupts.
    pub warnings: Vec<ScanWarning>,
}

/// A captured element value: the trimmed text plus the byte span of the
/// trimmed bytes (the edit target — surrounding whitespace is preserved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueField {
    /// The trimmed value text.
    pub text: String,
    /// Byte span of the trimmed text (inclusive start, exclusive end).
    pub span: Span,
}

/// The `<parent>` block: the coordinates the member's pom must match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParentRef {
    /// The parent's `<groupId>`.
    pub group_id: String,
    /// The parent's `<artifactId>`.
    pub artifact_id: String,
    /// The parent's `<version>` (as written).
    pub version: Option<String>,
}

/// One `<dependency>` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepRow {
    /// The dependency's `<groupId>` (absent = corrupt row; the adapter
    /// fails closed).
    pub group_id: Option<String>,
    /// The dependency's `<artifactId>`.
    pub artifact_id: Option<String>,
    /// The dependency's `<version>`, when the row carries one
    /// (dependency-managed rows carry none — verbatim, never resolved).
    pub version: Option<ValueField>,
    /// The dependency's `<scope>` (`test` maps to the dev edge kind).
    pub scope: Option<String>,
}

/// One scan observation (`DiscoveryWarning`'s raw form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanWarning {
    /// Stable warning tag (`ignored-section`).
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

/// Parses one pom.xml at the VERIFIED subset. Every failure is
/// fail-closed `E-DISC-MANIFEST` (the corrupt-pom posture — Maven itself
/// would refuse the build).
pub fn parse_pom(text: &str, file: &str) -> Result<Pom, DiscoverError> {
    let scanner = Scanner::new(text.as_bytes(), file);
    let pom = scanner.parse()?;
    if pom.artifact_id.is_none() {
        return Err(manifest_error(
            file,
            "pom has no <artifactId> — the coordinate's unique half is required",
            None,
        ));
    }
    Ok(pom)
}

fn manifest_error(file: &str, message: &str, span: Option<Span>) -> DiscoverError {
    DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Pom,
        message: message.to_owned(),
        span,
    }
}

/// The byte offset as a span coordinate (`u32`; a manifest beyond 4 GiB
/// is not a plausible input — the saturation keeps the arithmetic total).
fn offset(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// What a captured element collects, keyed by its open tag (the capture
/// context of its parent decides which names are interesting).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Capture {
    /// Not captured (consumed unmodeled).
    None,
    /// A coordinate field of `<project>` (`group_id`, `artifact_id`,
    /// `version`, `packaging`).
    ProjectField(&'static str),
    /// The `<parent>` block.
    Parent,
    /// A coordinate field of `<parent>`.
    ParentField(&'static str),
    /// The `<modules>` block.
    Modules,
    /// A `<module>` entry (its text).
    Module,
    /// The `<dependencies>` block.
    Dependencies,
    /// A `<dependency>` row.
    Dependency,
    /// A field of a `<dependency>` row.
    DependencyField(&'static str),
}

/// The element stack entry.
#[derive(Clone, Debug)]
struct Open {
    name: String,
    capture: Capture,
    /// Significant text accumulated so far (text nodes and CDATA),
    /// with byte offsets into the source.
    text: Vec<(usize, usize)>,
    /// A comment or PI inside the value: the captured bytes would not
    /// survive an edit untouched (I4), so the value is marked unplanable.
    tainted: bool,
}

struct Scanner<'a> {
    src: &'a [u8],
    file: &'a str,
    pos: usize,
    stack: Vec<Open>,
    pom: Pom,
    project_seen: bool,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a [u8], file: &'a str) -> Self {
        Self {
            src,
            file,
            pos: 0,
            stack: Vec::new(),
            pom: Pom::default(),
            project_seen: false,
        }
    }

    fn parse(mut self) -> Result<Pom, DiscoverError> {
        loop {
            self.skip_prose()?;
            if self.eof() {
                break;
            }
            if !self.take(b"<") {
                // `skip_prose` leaves the position on the next `<`.
                return Err(self.corrupt("expected an element"));
            }
            match self.peek_byte() {
                Some(b'/') => self.close_tag()?,
                Some(b'!') => self.skip_declaration()?,
                Some(b'?') => self.skip_pi()?,
                _ => self.open_tag()?,
            }
        }
        if let Some(top) = self.stack.last() {
            let top = top.name.clone();
            return Err(self.corrupt(&format!("unclosed <{top}> element")));
        }
        if !self.project_seen {
            return Err(self.corrupt("no <project> element (not a pom)"));
        }
        Ok(self.pom)
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    /// The unconsumed tail (panic-free by construction — `get` on a
    /// range, never a slice index).
    fn rest(&self) -> &[u8] {
        self.src.get(self.pos..).unwrap_or(&[])
    }

    fn corrupt(&self, message: &str) -> DiscoverError {
        let at = offset(self.pos.min(self.src.len()));
        manifest_error(self.file, message, Some(Span { start: at, end: at }))
    }

    /// Consumes the literal `bytes` when they sit at the cursor.
    fn take(&mut self, bytes: &[u8]) -> bool {
        if self.rest().starts_with(bytes) {
            self.pos += bytes.len();
            return true;
        }
        false
    }

    /// Skips whitespace, comments, the XML declaration, DOCTYPEs and
    /// processing instructions between elements, and records ordinary
    /// text plus CDATA as the current element's content. Text outside
    /// any element (prose at the top level) is consumed.
    fn skip_prose(&mut self) -> Result<(), DiscoverError> {
        let mut text_start: Option<usize> = None;
        while !self.eof() {
            if self.peek_byte() != Some(b'<') {
                text_start.get_or_insert(self.pos);
                self.pos += 1;
                continue;
            }
            if self.rest().starts_with(b"<!--") {
                self.taint_if_captured();
                self.skip_comment()?;
                continue;
            }
            if self.rest().starts_with(b"<![CDATA[") {
                // CDATA is significant content — but a value mixing
                // plain text and CDATA cannot be edited byte-safely
                // (I4): it is tainted instead.
                if text_start.is_some() {
                    self.taint_if_captured();
                }
                self.skip_cdata()?;
                text_start = None;
                continue;
            }
            if self.rest().starts_with(b"<?") {
                self.taint_if_captured();
                self.skip_pi()?;
                continue;
            }
            if self.rest().starts_with(b"<!") {
                self.taint_if_captured();
                self.skip_declaration()?;
                continue;
            }
            self.push_text(text_start.take());
            return Ok(());
        }
        self.push_text(text_start.take());
        Ok(())
    }

    /// Marks the innermost captured value element tainted (a comment or
    /// PI inside a captured value would be clobbered by an edit, I4).
    fn taint_if_captured(&mut self) {
        if let Some(open) = self.stack.last_mut()
            && !matches!(
                open.capture,
                Capture::None | Capture::Modules | Capture::Dependencies
            )
        {
            open.tainted = true;
        }
    }

    fn skip_comment(&mut self) -> Result<(), DiscoverError> {
        let start = self.pos;
        self.pos += 4;
        let Some(end) = find(self.src, self.pos, b"-->") else {
            self.pos = start;
            return Err(self.corrupt("unclosed comment"));
        };
        self.pos = end + 3;
        Ok(())
    }

    fn skip_pi(&mut self) -> Result<(), DiscoverError> {
        let start = self.pos;
        self.pos += 2;
        let Some(end) = find(self.src, self.pos, b"?>") else {
            self.pos = start;
            return Err(self.corrupt("unclosed processing instruction"));
        };
        self.pos = end + 2;
        Ok(())
    }

    /// Skips DOCTYPEs (and any other `<!...>` declaration, bracket-safe).
    fn skip_declaration(&mut self) -> Result<(), DiscoverError> {
        let start = self.pos;
        self.pos += 2;
        let mut depth = 0usize;
        while let Some(&b) = self.src.get(self.pos) {
            if b == b'<' {
                depth += 1;
            }
            if b == b'>' {
                if depth == 0 {
                    self.pos += 1;
                    return Ok(());
                }
                depth -= 1;
            }
            self.pos += 1;
        }
        self.pos = start;
        Err(self.corrupt("unclosed declaration"))
    }

    /// Consumes a CDATA section as significant text of the current
    /// element (`<![CDATA[...]]>`; the brackets never enter the value).
    fn skip_cdata(&mut self) -> Result<(), DiscoverError> {
        let open = self.pos;
        self.pos += 9;
        let Some(end) = find(self.src, self.pos, b"]]>") else {
            self.pos = open;
            return Err(self.corrupt("unclosed CDATA section"));
        };
        if let Some(elem) = self.stack.last_mut() {
            elem.text.push((self.pos, end));
        }
        self.pos = end + 3;
        Ok(())
    }

    /// Records the pending text range into the innermost captured
    /// element (trimmed bounds are computed at close time).
    fn push_text(&mut self, range: Option<usize>) {
        let Some(start) = range else {
            return;
        };
        let end = self.pos;
        if let Some(elem) = self.stack.last_mut()
            && start < end
        {
            elem.text.push((start, end));
        }
    }

    /// Opens one element: name, attributes (quoted values skipped),
    /// self-closing flag, capture slot.
    fn open_tag(&mut self) -> Result<(), DiscoverError> {
        let tag_start = self.pos;
        let Some(name) = self.tag_name() else {
            return Err(self.corrupt("malformed start tag"));
        };
        // Attributes: skip to the closing `>` respecting quotes.
        let mut quote: Option<u8> = None;
        let mut self_closing = false;
        loop {
            let Some(&b) = self.src.get(self.pos) else {
                self.pos = tag_start;
                return Err(self.corrupt("unclosed start tag"));
            };
            match (quote, b) {
                (Some(q), b) if b == q => {
                    quote = None;
                    self.pos += 1;
                }
                (None, b'"' | b'\'') => quote = Some(b),
                (None, b'>') => {
                    self.pos += 1;
                    break;
                }
                (None, b'/') if self.src.get(self.pos + 1) == Some(&b'>') => {
                    self_closing = true;
                    self.pos += 2;
                    break;
                }
                _ => self.pos += 1,
            }
        }
        let capture = self.capture_for(&name);
        if self_closing {
            // No content, no stack push (an empty capture consumes).
            return Ok(());
        }
        // Row-shaped captures pre-initialize their record at OPEN time:
        // the field elements fold before the row element closes.
        match capture {
            Capture::Parent => {
                self.pom.parent.get_or_insert(ParentRef {
                    group_id: String::new(),
                    artifact_id: String::new(),
                    version: None,
                });
            }
            Capture::Dependency => self.pom.dependencies.push(DepRow {
                group_id: None,
                artifact_id: None,
                version: None,
                scope: None,
            }),
            _ => {}
        }
        if name == "project" {
            if self.project_seen {
                return Err(self.corrupt("a nested <project> element"));
            }
            self.project_seen = true;
        }
        self.stack.push(Open {
            name,
            capture,
            text: Vec::new(),
            tainted: false,
        });
        Ok(())
    }

    /// The capture slot for a new element inside the current one.
    fn capture_for(&mut self, name: &str) -> Capture {
        let Some(parent) = self.stack.last() else {
            // The root element carries no value slot of its own — its
            // captured children build the pom.
            return Capture::None;
        };
        let in_project = matches!(parent.capture, Capture::None)
            && self.stack.len() == 1
            && parent.name == "project";
        match (&parent.capture, name) {
            // Direct children of <project> (a nested <project> is the
            // fallback's Capture::None — the open_tag guard refuses it).
            (Capture::None, "groupId") if in_project => Capture::ProjectField("group_id"),
            (Capture::None, "artifactId") if in_project => Capture::ProjectField("artifact_id"),
            (Capture::None, "version") if in_project => Capture::ProjectField("version"),
            (Capture::None, "packaging") if in_project => Capture::ProjectField("packaging"),
            (Capture::None, "parent") if in_project => Capture::Parent,
            (Capture::None, "modules") if in_project => Capture::Modules,
            (Capture::None, "dependencies") if in_project => Capture::Dependencies,
            (Capture::None, "profiles") if in_project => {
                self.pom.warnings.push(ScanWarning {
                    code: "ignored-section",
                    message: "<profiles> consumed unmodeled — profile-activated version \
                              overrides are build semantics a plan does not evaluate"
                        .to_owned(),
                });
                Capture::None
            }
            // Inside <parent>.
            (Capture::Parent, "groupId") => Capture::ParentField("group_id"),
            (Capture::Parent, "artifactId") => Capture::ParentField("artifact_id"),
            (Capture::Parent, "version") => Capture::ParentField("version"),
            // Inside <modules>.
            (Capture::Modules, "module") => Capture::Module,
            // Inside <dependencies>.
            (Capture::Dependencies, "dependency") => Capture::Dependency,
            // Inside <dependency>.
            (Capture::Dependency, "groupId") => Capture::DependencyField("group_id"),
            (Capture::Dependency, "artifactId") => Capture::DependencyField("artifact_id"),
            (Capture::Dependency, "version") => Capture::DependencyField("version"),
            (Capture::Dependency, "scope") => Capture::DependencyField("scope"),
            _ => Capture::None,
        }
    }

    fn tag_name(&mut self) -> Option<String> {
        let start = self.pos;
        while let Some(&b) = self.src.get(self.pos) {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' || b == b':' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        self.src
            .get(start..self.pos)
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .map(str::to_owned)
    }

    /// Closes the top element: validates the name, folds the captured
    /// value into the pom.
    fn close_tag(&mut self) -> Result<(), DiscoverError> {
        let close_start = self.pos;
        // The caller consumed the `<` (parse's `take`); skip the `/`.
        self.pos += 1;
        let Some(name) = self.tag_name() else {
            self.pos = close_start;
            return Err(self.corrupt("malformed end tag"));
        };
        // Skip to `>`.
        let Some(gt) = find(self.src, self.pos, b">") else {
            self.pos = close_start;
            return Err(self.corrupt("unclosed end tag"));
        };
        self.pos = gt + 1;
        let Some(open) = self.stack.pop() else {
            self.pos = close_start;
            return Err(self.corrupt(&format!("stray </{name}>")));
        };
        if open.name != name {
            return Err(manifest_error(
                self.file,
                &format!("mismatched element: </{name}> closes <{}>", open.name),
                Some(Span {
                    start: offset(close_start),
                    end: offset((gt + 1).min(self.src.len())),
                }),
            ));
        }
        self.fold(&open)?;
        Ok(())
    }

    /// Folds a closed element's captured text into the pom.
    fn fold(&mut self, open: &Open) -> Result<(), DiscoverError> {
        let value = self.field_of(open)?;
        match open.capture {
            Capture::None
            | Capture::Parent
            | Capture::Dependency
            | Capture::Dependencies
            | Capture::Modules => {}
            Capture::ProjectField(field) => {
                // groupId/version/packaging are optional at the grammar
                // (inheritance / the jar default); only artifactId is
                // required — enforced once at parse_pom.
                if let Some(value) = value {
                    match field {
                        "group_id" => self.pom.group_id = Some(value.text),
                        "artifact_id" => self.pom.artifact_id = Some(value.text),
                        "version" => self.pom.version = Some(value),
                        "packaging" => self.pom.packaging = Some(value.text),
                        _ => {}
                    }
                }
            }
            Capture::ParentField(field) => {
                let value = value.ok_or_else(|| {
                    manifest_error(
                        self.file,
                        &format!("<parent>/<{}> carries no value", field_label(field)),
                        None,
                    )
                })?;
                let Some(parent) = self.pom.parent.as_mut() else {
                    return Err(manifest_error(
                        self.file,
                        "a <parent> field closed outside a <parent> block — corrupt pom",
                        None,
                    ));
                };
                match field {
                    "group_id" => parent.group_id = value.text,
                    "artifact_id" => parent.artifact_id = value.text,
                    "version" => parent.version = Some(value.text),
                    _ => {}
                }
            }
            Capture::Module => {
                let value = value
                    .ok_or_else(|| manifest_error(self.file, "<module> carries no value", None))?;
                self.pom.modules.push(value.text);
            }
            Capture::DependencyField(field) => {
                let value = value.ok_or_else(|| {
                    manifest_error(
                        self.file,
                        &format!("<dependency>/<{}> carries no value", field_label(field)),
                        None,
                    )
                })?;
                let Some(row) = self.pom.dependencies.last_mut() else {
                    return Err(manifest_error(
                        self.file,
                        "a <dependency> field closed outside a <dependency> row — corrupt pom",
                        None,
                    ));
                };
                match field {
                    "group_id" => row.group_id = Some(value.text),
                    "artifact_id" => row.artifact_id = Some(value.text),
                    "version" => row.version = Some(value),
                    "scope" => row.scope = Some(value.text),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// The captured value of one element: trimmed text + trimmed span;
    /// `None` for empty/whitespace or tainted values.
    fn field_of(&mut self, open: &Open) -> Result<Option<ValueField>, DiscoverError> {
        if open.tainted {
            self.pom.warnings.push(ScanWarning {
                code: "ignored-section",
                message: format!(
                    "<{}> carries a comment or processing instruction inside the value — the \
                     element is consumed unmodeled (an edit could not preserve its bytes)",
                    open.name
                ),
            });
            return Ok(None);
        }
        let (Some(&(first_start, _)), Some(&(_, last_end))) = (open.text.first(), open.text.last())
        else {
            return Ok(None);
        };
        let raw = self
            .src
            .get(first_start..last_end)
            .ok_or_else(|| manifest_error(self.file, "element value is out of bounds", None))?;
        let text = std::str::from_utf8(raw)
            .map_err(|_| manifest_error(self.file, "element value is not valid UTF-8", None))?
            .trim();
        if text.is_empty() {
            return Ok(None);
        }
        // Trimmed bounds (byte offsets of the first/last significant
        // byte).
        let lead = raw
            .iter()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(0);
        let trail = raw
            .iter()
            .rposition(|b| !b.is_ascii_whitespace())
            .unwrap_or(0);
        let span = Span {
            start: offset(first_start + lead),
            end: offset((first_start + trail + 1).min(self.src.len())),
        };
        Ok(Some(ValueField {
            text: text.to_owned(),
            span,
        }))
    }
}

fn field_label(field: &str) -> &'static str {
    match field {
        "group_id" => "groupId",
        "artifact_id" => "artifactId",
        "version" => "version",
        "packaging" => "packaging",
        "scope" => "scope",
        _ => "?",
    }
}

/// Finds `needle` in `src[from..]` (byte offset of the first byte);
/// panic-free by construction (`windows` over a `get`-ed slice).
fn find(src: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let hay = src.get(from..)?;
    hay.windows(needle.len())
        .position(|window| window == needle)
        .map(|index| index + from)
}

#[cfg(test)]
mod diag {
    use super::*;

    #[test]
    fn parses_a_minimal_pom() {
        let pom = parse_pom(
            "<project xmlns=\"u\"><groupId>g</groupId><artifactId>a</artifactId><version>1.2.3</version></project>",
            "pom.xml",
        )
        .expect("parses");
        assert_eq!(pom.artifact_id.as_deref(), Some("a"));
    }

    #[test]
    fn a_comment_inside_the_version_taints_the_value() {
        let pom = parse_pom(
            "<project><artifactId>a</artifactId><version>1<!--x-->2</version></project>",
            "pom.xml",
        )
        .expect("parses");
        assert!(pom.version.is_none(), "the tainted value plans no version");
        assert!(
            pom.warnings.iter().any(|w| w.code == "ignored-section"),
            "the taint is warned"
        );
    }

    #[test]
    fn a_mismatched_close_refuses() {
        let err = parse_pom("<project><groupId>g</artifactId></project>", "pom.xml")
            .expect_err("mismatched tags refuse");
        assert_eq!(err.code(), "E-DISC-MANIFEST");
    }

    #[test]
    fn self_closing_and_cdata_shapes_parse() {
        let pom = parse_pom(
            "<project><artifactId>a</artifactId><packaging/>\
             <version><![CDATA[1.2.3]]></version><modules><module>m</module></modules></project>",
            "pom.xml",
        )
        .expect("parses");
        assert!(
            pom.packaging.is_none(),
            "a self-closing element carries no value slot"
        );
        assert_eq!(
            pom.version.as_ref().map(|v| v.text.as_str()),
            Some("1.2.3"),
            "CDATA content is the value"
        );
        assert_eq!(pom.modules, vec!["m".to_owned()]);
    }
}
