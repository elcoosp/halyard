//! The `.csproj` `MSBuild` grammar (M8 ADR-0059), hand-rolled over the
//! VERIFIED reference (`docs/research/nuget-grammars.md`, from
//! `learn.microsoft.com`: the pack-target property table, the
//! package-versioning reference, the project-SDK overview and the
//! customize-by-directory page):
//!
//! - the SDK declaration: *"The `Project` element has an optional `Sdk`
//!   attribute"* or the top-level *"`Sdk` element"* — a project file
//!   declaring neither is consumed unmodeled (a classic project's
//!   metadata may live outside the file, §1 of the research note);
//! - the pack version: *"`PackageVersion` — ... Default is the value of
//!   `$(Version)`"* and *"Defaults to `Version` if not set"* — the
//!   scanner captures `PackageVersion`, `Version`, `VersionPrefix`,
//!   `VersionSuffix`, `PackageId`, `AssemblyName` and `IsPackable` as
//!   `<PropertyGroup>` property elements with their value spans;
//! - the references: `<PackageReference Include=... Version=... />` and
//!   `<ProjectReference Include=... />` are attribute-shaped rows (the
//!   documented examples) — the scanner captures attribute values with
//!   their byte spans;
//! - conditions: *"Conditions can also be applied at the `ItemGroup`
//!   level"* — a conditioned `Project`/`PropertyGroup`/`ItemGroup` or
//!   property/reference element is consumed unmodeled with a warning;
//! - overrides: *"The property might be getting overwritten later"* —
//!   a re-declared property follows the later declaration.
//!
//! The scanner is offset-preserving like the sibling hand-written
//! lexers (ADR-0001's JSON scanner, ADR-0057's go.mod lexer, ADR-0058's
//! pom scanner): spans point into the ORIGINAL bytes, whitespace and
//! comments outside the captured values never move (invariant I4).
//! Comments, the XML declaration, DOCTYPEs, processing instructions and
//! CDATA are consumed; a comment *inside* a captured value makes that
//! value unplanable (the value's bytes would not survive an edit
//! untouched). XML entity references ride verbatim (the pom posture —
//! a decoded value would not round-trip the bytes).

use halyard_core::discover::{DiscoverError, ManifestFormat};
use halyard_core::package::Span;

/// One parsed `.csproj` (the planning-relevant subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Csproj {
    /// The declared SDK (the `Sdk` attribute value or the top-level
    /// `<Sdk Name>` value) — `None` for a classic (non-SDK) project.
    pub sdk: Option<String>,
    /// Whether the root `Project` element carries a `Condition`
    /// attribute (the whole file may not apply — the adapter consumes
    /// it unmodeled).
    pub conditioned_root: bool,
    /// The `<PackageId>` property (the identity chain's head).
    pub package_id: Option<ValueField>,
    /// The `<AssemblyName>` property (the chain's middle step).
    pub assembly_name: Option<ValueField>,
    /// The `<PackageVersion>` property (the pack target's head).
    pub package_version: Option<ValueField>,
    /// The `<Version>` property (the `$(Version)` fallback).
    pub version: Option<ValueField>,
    /// The `<VersionPrefix>` property (the composite half — never
    /// planned alone).
    pub version_prefix: Option<ValueField>,
    /// The `<VersionSuffix>` property (the composite half — never
    /// planned alone).
    pub version_suffix: Option<ValueField>,
    /// `<IsPackable>false</IsPackable>` (the documented default is
    /// `true`).
    pub unpackable: bool,
    /// `<PackageReference>` rows, document order.
    pub package_refs: Vec<PackageRef>,
    /// `<ProjectReference>` rows, document order.
    pub project_refs: Vec<ProjectRef>,
    /// Non-fatal observations (§6.3): conditioned blocks consumed
    /// unmodeled, values a comment corrupts.
    pub warnings: Vec<ScanWarning>,
}

/// A captured value: the trimmed text plus the byte span of the trimmed
/// bytes (the edit target — surrounding whitespace is preserved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueField {
    /// The trimmed value text.
    pub text: String,
    /// Byte span of the trimmed text (inclusive start, exclusive end).
    pub span: Span,
}

/// One `<PackageReference>` row (attributes captured verbatim).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageRef {
    /// The `Include` attribute (the package identity; corrupt when
    /// absent — the adapter fails closed).
    pub include: Option<ValueField>,
    /// The `Version` attribute (the range text; absent for
    /// central-package-management rows — consumed unmodeled).
    pub version: Option<ValueField>,
}

/// One `<ProjectReference>` row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRef {
    /// The `Include` attribute (the referenced project's path).
    pub include: Option<ValueField>,
}

/// One scan observation (`DiscoveryWarning`'s raw form).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanWarning {
    /// Stable warning tag (`ignored-section`).
    pub code: &'static str,
    /// Human-readable detail.
    pub message: String,
}

/// Parses one `.csproj` at the VERIFIED subset. Every failure is
/// fail-closed `E-DISC-MANIFEST` (the corrupt-project posture — `MSBuild`
/// itself would refuse the file).
pub fn parse_csproj(text: &str, file: &str) -> Result<Csproj, DiscoverError> {
    let scanner = Scanner::new(text.as_bytes(), file);
    scanner.parse()
}

fn manifest_error(file: &str, message: &str, span: Option<Span>) -> DiscoverError {
    DiscoverError::Manifest {
        file: file.to_owned(),
        format: ManifestFormat::Csproj,
        message: message.to_owned(),
        span,
    }
}

/// The byte offset as a span coordinate (`u32`; a manifest beyond 4 GiB
/// is not a plausible input — the saturation keeps the arithmetic total).
fn offset(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The property slots the scanner captures (exact `MSBuild` names — the
/// documented `PascalCase` spellings of the pack-target table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropertySlot {
    PackageId,
    AssemblyName,
    PackageVersion,
    Version,
    VersionPrefix,
    VersionSuffix,
    IsPackable,
}

impl PropertySlot {
    fn of(name: &str) -> Option<Self> {
        match name {
            "PackageId" => Some(Self::PackageId),
            "AssemblyName" => Some(Self::AssemblyName),
            "PackageVersion" => Some(Self::PackageVersion),
            "Version" => Some(Self::Version),
            "VersionPrefix" => Some(Self::VersionPrefix),
            "VersionSuffix" => Some(Self::VersionSuffix),
            "IsPackable" => Some(Self::IsPackable),
            _ => None,
        }
    }
}

/// What a captured element collects, keyed by its open tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Capture {
    /// Not captured (consumed unmodeled).
    None,
    /// The root `<Project>` element.
    Project,
    /// The top-level `<Sdk>` element.
    SdkChild,
    /// A `<PropertyGroup>` block.
    PropertyGroup,
    /// A property element inside a `<PropertyGroup>`.
    Property(PropertySlot),
    /// An `<ItemGroup>` block.
    ItemGroup,
    /// A `<PackageReference>` attribute row.
    PackageRef,
    /// A `<ProjectReference>` attribute row.
    ProjectRef,
}

/// An attribute captured at open time: name + value (verbatim text and
/// byte span).
#[derive(Clone, Debug)]
struct AttrField {
    name: String,
    value: ValueField,
}

/// The element stack entry.
#[derive(Debug)]
struct Open {
    name: String,
    capture: Capture,
    /// Attributes captured at open time (quoted values verbatim).
    attrs: Vec<AttrField>,
    /// Whether the element carries a `Condition` attribute.
    conditioned: bool,
    /// Significant text accumulated so far (text nodes and CDATA), with
    /// byte offsets into the source.
    text: Vec<(usize, usize)>,
    /// A comment or PI inside the value: the captured bytes would not
    /// survive an edit untouched (I4), so the value is marked unplanable.
    tainted: bool,
}

struct Scanner<'a> {
    src: &'a [u8],
    file: &'a str,
    pos: usize,
    /// The self-closing flag of the start tag `attrs` just consumed.
    self_closing: bool,
    stack: Vec<Open>,
    csproj: Csproj,
    project_seen: bool,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a [u8], file: &'a str) -> Self {
        Self {
            src,
            file,
            pos: 0,
            self_closing: false,
            stack: Vec::new(),
            csproj: Csproj::default(),
            project_seen: false,
        }
    }

    fn parse(mut self) -> Result<Csproj, DiscoverError> {
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
            return Err(self.corrupt("no <Project> element (not an MSBuild project)"));
        }
        Ok(self.csproj)
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
                Capture::None | Capture::Project | Capture::SdkChild
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

    /// Opens one element: name, attributes (captured verbatim), the
    /// condition flag, self-closing handling, capture slot.
    fn open_tag(&mut self) -> Result<(), DiscoverError> {
        let Some(name) = self.tag_name() else {
            return Err(self.corrupt("malformed start tag"));
        };
        let attrs = self.attrs()?;
        let conditioned = attrs.iter().any(|attr| attr.name == "Condition");
        let self_closing = self.self_closing;
        let capture = self.capture_for(&name, &attrs, conditioned);
        if name == "Project" {
            if self.project_seen {
                return Err(self.corrupt("a nested <Project> element"));
            }
            self.project_seen = true;
            self.csproj.conditioned_root = conditioned;
            if self.csproj.sdk.is_none() {
                self.csproj.sdk = attrs
                    .iter()
                    .find(|attr| attr.name == "Sdk")
                    .map(|attr| attr.value.text.clone());
            }
        }
        match capture {
            Capture::PackageRef | Capture::ProjectRef => {
                if self_closing {
                    // No element body: the attribute row is complete at
                    // once (a fresh record — nothing was pre-initialized).
                    self.push_row_from_attrs(capture, &attrs);
                    return Ok(());
                }
                // Row-shaped captures pre-initialize their record at
                // OPEN time; the (metadata) children fold before the
                // row element closes.
                if capture == Capture::PackageRef {
                    self.csproj.package_refs.push(PackageRef {
                        include: None,
                        version: None,
                    });
                } else {
                    self.csproj.project_refs.push(ProjectRef { include: None });
                }
            }
            _ => {}
        }
        if self_closing {
            // No content, no stack push (an empty capture consumes).
            return Ok(());
        }
        self.stack.push(Open {
            name,
            capture,
            attrs,
            conditioned,
            text: Vec::new(),
            tainted: false,
        });
        Ok(())
    }

    /// Scans the attribute list after the tag name, up to `>` or `/>`
    /// (respecting quotes), capturing `name="value"` pairs verbatim.
    fn attrs(&mut self) -> Result<Vec<AttrField>, DiscoverError> {
        let mut attrs = Vec::new();
        self.self_closing = false;
        let mut quote: Option<u8> = None;
        loop {
            let Some(&b) = self.src.get(self.pos) else {
                return Err(self.corrupt("unclosed start tag"));
            };
            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
                self.pos += 1;
                continue;
            }
            match b {
                b'"' | b'\'' => {
                    quote = Some(b);
                    self.pos += 1;
                }
                b'>' => {
                    self.pos += 1;
                    return Ok(attrs);
                }
                b'/' if self.src.get(self.pos + 1) == Some(&b'>') => {
                    self.pos += 2;
                    self.self_closing = true;
                    return Ok(attrs);
                }
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                _ => {
                    if let Some(attr) = self.attr_field()? {
                        attrs.push(attr);
                    } else {
                        // A malformed attribute token: consumed to the
                        // next delimiter (MSBuild would refuse the
                        // file; nothing here guesses).
                        self.pos += 1;
                    }
                }
            }
        }
    }

    /// Captures one `name="value"` attribute pair at the cursor.
    fn attr_field(&mut self) -> Result<Option<AttrField>, DiscoverError> {
        let name_start = self.pos;
        while let Some(&b) = self.src.get(self.pos) {
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == name_start {
            return Ok(None);
        }
        let name = self
            .src
            .get(name_start..self.pos)
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .map_or(String::new(), str::to_owned);
        if self.peek_byte() != Some(b'=') {
            return Ok(None);
        }
        self.pos += 1;
        let Some(&quote) = self.src.get(self.pos) else {
            return Err(self.corrupt("unclosed start tag"));
        };
        if quote != b'"' && quote != b'\'' {
            return Err(self.corrupt("an attribute value must be quoted"));
        }
        self.pos += 1;
        let value_start = self.pos;
        while let Some(&b) = self.src.get(self.pos) {
            if b == quote {
                break;
            }
            self.pos += 1;
        }
        if self.peek_byte() != Some(quote) {
            return Err(self.corrupt("unclosed attribute value"));
        }
        let value_end = self.pos;
        self.pos += 1;
        let raw = self
            .src
            .get(value_start..value_end)
            .ok_or_else(|| self.corrupt("attribute value is out of bounds"))?;
        let text = std::str::from_utf8(raw)
            .map_err(|_| self.corrupt("attribute value is not valid UTF-8"))?
            .to_owned();
        Ok(Some(AttrField {
            name,
            value: ValueField {
                text,
                span: Span {
                    start: offset(value_start),
                    end: offset(value_end),
                },
            },
        }))
    }

    /// The capture slot for a new element inside the current one.
    fn capture_for(&mut self, name: &str, attrs: &[AttrField], conditioned: bool) -> Capture {
        let Some(parent) = self.stack.last() else {
            // The root element carries no value slot of its own — its
            // captured children build the csproj.
            return Capture::Project;
        };
        let parent_conditioned = parent.conditioned;
        match (&parent.capture, name) {
            // Direct children of <Project> (a nested <Project> is the
            // fallback's Capture::Project — the open_tag guard refuses
            // it).
            (_, "Project") => Capture::Project,
            (Capture::Project, "Sdk") => {
                match attrs.iter().find(|attr| attr.name == "Name") {
                    Some(attr) => {
                        if self.csproj.sdk.is_none() {
                            self.csproj.sdk = Some(attr.value.text.clone());
                        }
                    }
                    None => self.csproj.warnings.push(ScanWarning {
                        code: "ignored-section",
                        message: "<Sdk> carries no Name attribute — consumed unmodeled".to_owned(),
                    }),
                }
                Capture::SdkChild
            }
            (Capture::Project, "PropertyGroup") => Capture::PropertyGroup,
            (Capture::Project, "ItemGroup") => Capture::ItemGroup,
            (Capture::PropertyGroup, _) => {
                if parent_conditioned {
                    if PropertySlot::of(name).is_some() {
                        self.csproj.warnings.push(ScanWarning {
                            code: "ignored-section",
                            message: format!(
                                "<{name}> sits in a conditioned <PropertyGroup> — consumed \
                                 unmodeled (the value may not be the pack-time one)"
                            ),
                        });
                    }
                    return Capture::None;
                }
                if conditioned {
                    self.csproj.warnings.push(ScanWarning {
                        code: "ignored-section",
                        message: format!(
                            "<{name}> carries a Condition — consumed unmodeled (the value \
                             may not be the pack-time one)"
                        ),
                    });
                    return Capture::None;
                }
                match PropertySlot::of(name) {
                    Some(slot) => Capture::Property(slot),
                    None => Capture::None,
                }
            }
            (Capture::ItemGroup, "PackageReference" | "ProjectReference") => {
                if parent_conditioned || conditioned {
                    self.csproj.warnings.push(ScanWarning {
                        code: "ignored-section",
                        message: format!(
                            "<{name}> is conditioned — consumed unmodeled (the reference \
                             may not apply at pack time)"
                        ),
                    });
                    return Capture::None;
                }
                if name == "PackageReference" {
                    Capture::PackageRef
                } else {
                    Capture::ProjectRef
                }
            }
            _ => Capture::None,
        }
    }

    /// Pushes one complete attribute-shaped reference row (the
    /// self-closing form: no record was pre-initialized).
    fn push_row_from_attrs(&mut self, capture: Capture, attrs: &[AttrField]) {
        let find_attr = |want: &str| {
            attrs
                .iter()
                .find(|attr| attr.name == want)
                .map(|attr| attr.value.clone())
        };
        match capture {
            Capture::PackageRef => self.csproj.package_refs.push(PackageRef {
                include: find_attr("Include"),
                version: find_attr("Version"),
            }),
            Capture::ProjectRef => self.csproj.project_refs.push(ProjectRef {
                include: find_attr("Include"),
            }),
            _ => {}
        }
    }

    /// Completes one pre-initialized attribute-shaped reference row
    /// from its open-time attributes (the element-body form).
    fn complete_row(&mut self, capture: Capture, attrs: &[AttrField]) {
        let find_attr = |want: &str| {
            attrs
                .iter()
                .find(|attr| attr.name == want)
                .map(|attr| attr.value.clone())
        };
        match capture {
            Capture::PackageRef => {
                if let Some(row) = self.csproj.package_refs.last_mut() {
                    row.include = find_attr("Include");
                    row.version = find_attr("Version");
                }
            }
            Capture::ProjectRef => {
                if let Some(row) = self.csproj.project_refs.last_mut() {
                    row.include = find_attr("Include");
                }
            }
            _ => {}
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
    /// value into the csproj.
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

    /// Folds a closed element's captured text (or completes its
    /// attribute row) into the csproj.
    fn fold(&mut self, open: &Open) -> Result<(), DiscoverError> {
        match open.capture {
            Capture::None
            | Capture::Project
            | Capture::SdkChild
            | Capture::PropertyGroup
            | Capture::ItemGroup => {}
            Capture::PackageRef | Capture::ProjectRef => {
                if !open.conditioned {
                    self.complete_row(open.capture, &open.attrs);
                }
            }
            Capture::Property(slot) => {
                let value = self.field_of(open)?;
                if let Some(value) = value {
                    self.fold_property(slot, value);
                }
            }
        }
        Ok(())
    }

    /// Folds one property value into its slot (the later declaration
    /// wins — the documented override rule).
    fn fold_property(&mut self, slot: PropertySlot, value: ValueField) {
        match slot {
            PropertySlot::PackageId => self.csproj.package_id = Some(value),
            PropertySlot::AssemblyName => self.csproj.assembly_name = Some(value),
            PropertySlot::PackageVersion => self.csproj.package_version = Some(value),
            PropertySlot::Version => self.csproj.version = Some(value),
            PropertySlot::VersionPrefix => self.csproj.version_prefix = Some(value),
            PropertySlot::VersionSuffix => self.csproj.version_suffix = Some(value),
            PropertySlot::IsPackable => {
                self.csproj.unpackable = value.text.eq_ignore_ascii_case("false");
            }
        }
    }

    /// The captured value of one element: trimmed text + trimmed span;
    /// `None` for empty/whitespace or tainted values.
    fn field_of(&mut self, open: &Open) -> Result<Option<ValueField>, DiscoverError> {
        if open.tainted {
            self.csproj.warnings.push(ScanWarning {
                code: "ignored-section",
                message: format!(
                    "<{}> carries a comment or processing instruction inside the value — \
                     the element is consumed unmodeled (an edit could not preserve its \
                     bytes)",
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
            .ok_or_else(|| self.corrupt("element value is out of bounds"))?;
        let text = std::str::from_utf8(raw)
            .map_err(|_| self.corrupt("element value is not valid UTF-8"))?
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
    fn parses_a_minimal_sdk_style_project() {
        let csproj = parse_csproj(
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><Version>1.2.3</Version>\
             </PropertyGroup></Project>",
            "a.csproj",
        )
        .expect("parses");
        assert_eq!(csproj.sdk.as_deref(), Some("Microsoft.NET.Sdk"));
        assert_eq!(
            csproj.version.as_ref().map(|v| v.text.as_str()),
            Some("1.2.3")
        );
    }

    #[test]
    fn a_comment_inside_the_version_taints_the_value() {
        let csproj = parse_csproj(
            "<Project Sdk=\"S\"><PropertyGroup><Version>1<!--x-->2</Version></PropertyGroup>\
             </Project>",
            "a.csproj",
        )
        .expect("parses");
        assert!(
            csproj.version.is_none(),
            "the tainted value plans no version"
        );
        assert!(
            csproj.warnings.iter().any(|w| w.code == "ignored-section"),
            "the taint is warned"
        );
    }

    #[test]
    fn a_mismatched_close_refuses() {
        let err = parse_csproj(
            "<Project Sdk=\"S\"><PropertyGroup><Version>1</PackageVersion></PropertyGroup>\
             </Project>",
            "a.csproj",
        )
        .expect_err("mismatched tags refuse");
        assert_eq!(err.code(), "E-DISC-MANIFEST");
    }

    #[test]
    fn self_closing_reference_rows_capture_their_attributes() {
        let csproj = parse_csproj(
            "<Project Sdk=\"S\">\
             <ItemGroup>\
             <PackageReference Include=\"Some.Lib\" Version=\"1.0.0\" />\
             <ProjectReference Include=\"..\\lib\\Lib.csproj\" />\
             </ItemGroup></Project>",
            "a.csproj",
        )
        .expect("parses");
        assert_eq!(csproj.package_refs.len(), 1);
        assert_eq!(
            csproj
                .package_refs
                .first()
                .expect("row")
                .include
                .as_ref()
                .map(|v| v.text.as_str()),
            Some("Some.Lib")
        );
        assert_eq!(
            csproj
                .package_refs
                .first()
                .expect("row")
                .version
                .as_ref()
                .map(|v| v.text.as_str()),
            Some("1.0.0")
        );
        assert_eq!(csproj.project_refs.len(), 1);
        assert_eq!(
            csproj
                .project_refs
                .first()
                .expect("row")
                .include
                .as_ref()
                .map(|v| v.text.as_str()),
            Some("..\\lib\\Lib.csproj")
        );
    }

    #[test]
    fn a_conditioned_item_group_consumes_its_references() {
        let csproj = parse_csproj(
            "<Project Sdk=\"S\">\
             <ItemGroup Condition=\"'$(Configuration)' == 'Release'\">\
             <PackageReference Include=\"Some.Lib\" Version=\"1.0.0\" />\
             </ItemGroup>\
             <ItemGroup>\
             <PackageReference Include=\"Other.Lib\" Version=\"2.0.0\" />\
             </ItemGroup>\
             </Project>",
            "a.csproj",
        )
        .expect("parses");
        assert_eq!(
            csproj.package_refs.len(),
            1,
            "only the unconditioned reference is a row"
        );
        assert_eq!(
            csproj
                .package_refs
                .first()
                .expect("row")
                .include
                .as_ref()
                .map(|v| v.text.as_str()),
            Some("Other.Lib")
        );
        assert!(
            csproj.warnings.iter().any(|w| w.code == "ignored-section"),
            "the conditioned block is warned"
        );
    }
}
