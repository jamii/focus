//! Snapshots the non-private items of every module in the two lib crates, so
//! that a change to the API surface shows up as a snapshot diff. What each
//! crate exports and what it only shares internally are snapshotted
//! separately, and every item, field and method lands in exactly one of them.

use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use syn::parse::Parser as _;

const PUBLIC_HEADER: &str = "\
# Items exported from the crates
#
# The public API of focus-core and focus: `pub` items in modules that are
# themselves `pub` all the way up to the crate root, with their `pub` fields
# and methods. Everything else is in api_internal.snap.
#
# Types are listed in full, except for fields that are not exported;
# functions, consts and statics are listed as signatures only, and trait
# impls as a header line without their methods. Doc comments are stripped.
#
# Update intentionally with:
#   UPDATE_API=1 cargo test -p focus-core --test api
";

const INTERNAL_HEADER: &str = "\
# Items shared inside the crates
#
# Every module of focus-core and focus, with the items, fields and methods
# that are visible outside their own module but are not exported from the
# crate: `pub(crate)` and `pub(super)`, and `pub` inside a module that isn't
# itself exported. What the crates do export is in api_public.snap, and a
# type with both kinds of member appears in both files, carrying the members
# that belong to each.
#
# Same conventions as api_public.snap: whole types, signatures for the rest.
#
# Update intentionally with:
#   UPDATE_API=1 cargo test -p focus-core --test api
";

/// Crate name and the path of its lib target, relative to the workspace.
const CRATES: [(&str, &str); 2] = [
    ("focus_core", "focus-core/src/lib.rs"),
    ("focus", "focus/src/lib.rs"),
];

/// Stands in for fields left out of a snapshot. Used as a field name and as a
/// field type, so that both named and tuple structs can carry one.
const ELIDED_FIELDS: &str = "__ELIDED_FIELDS";

/// Stands in for the value of a const or static.
const ELIDED_VALUE: &str = "__ELIDED_VALUE";

/// Which of the two snapshots is being rendered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    /// Reachable from outside the crate.
    Public,
    /// Visible outside its own module, but not from outside the crate.
    Internal,
}

struct Module {
    /// eg `focus_core::page`.
    path: String,
    /// eg `pub mod`, or `crate` for a lib root.
    declaration: String,
    /// Whether this module is `pub` all the way up to the crate root, so that
    /// its `pub` items are exported.
    exported: bool,
    items: Vec<syn::Item>,
}

#[test]
fn api_matches_snapshot() {
    let core_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = core_dir.parent().unwrap();

    let mut modules = Vec::new();
    for (crate_name, lib_path) in CRATES {
        let lib_path = workspace_dir.join(lib_path);
        let src_dir = lib_path.parent().unwrap().to_owned();
        collect_modules(
            &src_dir,
            crate_name.to_owned(),
            "crate".to_owned(),
            true,
            parse(&lib_path),
            &mut modules,
        );
    }
    modules.sort_by(|left, right| left.path.cmp(&right.path));

    for (surface, header, name) in [
        (Surface::Public, PUBLIC_HEADER, "tests/api_public.snap"),
        (
            Surface::Internal,
            INTERNAL_HEADER,
            "tests/api_internal.snap",
        ),
    ] {
        let mut actual = header.to_owned();
        for module in &modules {
            // A module that isn't exported has no public surface at all.
            if surface == Surface::Public && !module.exported {
                continue;
            }
            actual.push_str(&render_module(module, surface));
        }
        assert_snapshot(&core_dir.join(name), &actual);
    }
}

fn assert_snapshot(path: &Path, actual: &str) {
    if env::var_os("UPDATE_API").is_some() {
        std::fs::write(path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "failed to read {}: {error}; create it with UPDATE_API=1",
            path.display()
        )
    });
    assert_eq!(expected, actual, "{} is out of date", path.display());
}

fn parse(path: &Path) -> Vec<syn::Item> {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    parse_source(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn parse_source(source: &str) -> syn::Result<Vec<syn::Item>> {
    // Doc comments are dropped here rather than from the parsed items,
    // because they attach to fields and variants as well as to items.
    let source: String = source
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("///") && !line.starts_with("//!")
        })
        .map(|line| format!("{line}\n"))
        .collect();
    Ok(syn::parse_file(&source)?.items)
}

/// Records `items` as the module at `path`, then descends into the modules it
/// declares. `dir` is the directory that `mod foo;` resolves against.
fn collect_modules(
    dir: &Path,
    path: String,
    declaration: String,
    exported: bool,
    items: Vec<syn::Item>,
    modules: &mut Vec<Module>,
) {
    for item in &items {
        let syn::Item::Mod(module) = item else {
            continue;
        };
        if is_cfg_test(&module.attrs) {
            continue;
        }
        let name = module.ident.to_string();
        let child_dir = dir.join(&name);
        let child_path = format!("{path}::{name}");
        let child_declaration = match visibility(&module.vis) {
            Some(visibility) => format!("{visibility} mod"),
            None => "mod".to_owned(),
        };
        let child_items = match &module.content {
            Some((_, items)) => items.clone(),
            None => {
                let as_file = dir.join(format!("{name}.rs"));
                let file = if as_file.exists() {
                    as_file
                } else {
                    child_dir.join("mod.rs")
                };
                parse(&file)
            }
        };
        collect_modules(
            &child_dir,
            child_path,
            child_declaration,
            exported && is_public(&module.vis),
            child_items,
            modules,
        );
    }
    modules.push(Module {
        path,
        declaration,
        exported,
        items,
    });
}

fn render_module(module: &Module, surface: Surface) -> String {
    let mut output = format!("\n## {} {}\n", module.declaration, module.path);
    let mut previous_was_multiline = true;
    for item in &module.items {
        let Some(rendered) = render_item(item, module, surface) else {
            continue;
        };
        // Blank lines around anything that spans lines, and around impls;
        // runs of one-liners, like a block of re-exports, stay together.
        let multiline = rendered.contains('\n') || matches!(item, syn::Item::Impl(_));
        if multiline || previous_was_multiline {
            output.push('\n');
        }
        writeln!(output, "{rendered}").unwrap();
        previous_was_multiline = multiline;
    }
    output
}

/// The snapshot form of one item on `surface`, or `None` if the item has
/// nothing on that surface, is test-only, or is a module (which gets a
/// section of its own).
fn render_item(item: &syn::Item, module: &Module, surface: Surface) -> Option<String> {
    if item_attrs(item).is_some_and(is_cfg_test) {
        return None;
    }
    match item {
        syn::Item::Mod(_) => None,
        syn::Item::Impl(item) => render_impl(item, module, surface),
        syn::Item::Macro(item) => {
            // `macro_rules!` has no visibility; exporting is what makes it
            // reachable, and it exports from the crate root wherever it is.
            let exported = item
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("macro_export"));
            let name = item.ident.as_ref()?;
            (exported && surface == Surface::Public)
                .then(|| format!("#[macro_export]\nmacro_rules! {name} {{ /* ... */ }}"))
        }
        _ => render_plain_item(item, module.exported, surface),
    }
}

/// An item that isn't an impl: kept whole, with its fields split by surface.
fn render_plain_item(item: &syn::Item, in_exported: bool, surface: Surface) -> Option<String> {
    let vis = item_visibility(item)?;
    let kept = keep(vis, in_exported, surface);
    // A type can be exported while some of its fields are only internal.
    let exported = in_exported && is_public(vis);
    match item {
        syn::Item::Fn(item) if kept => {
            let mut item = item.clone();
            item.block = Box::new(empty_block());
            Some(signatures_only(&unparse(syn::Item::Fn(item))))
        }
        syn::Item::Trait(item) if kept => {
            let mut item = item.clone();
            for item in &mut item.items {
                if let syn::TraitItem::Fn(item) = item {
                    item.default = None;
                    item.semi_token = Some(Default::default());
                }
            }
            Some(unparse(syn::Item::Trait(item)))
        }
        syn::Item::Struct(item) => {
            let mut item = item.clone();
            let elision = elide_fields(&mut item.fields, exported, surface)?;
            (kept || elision.kept_any)
                .then(|| mark_elisions(&unparse(syn::Item::Struct(item)), elision.marker))
        }
        syn::Item::Union(item) => {
            let mut item = item.clone();
            let mut fields = syn::Fields::Named(item.fields.clone());
            let elision = elide_fields(&mut fields, exported, surface)?;
            let syn::Fields::Named(named) = fields else {
                unreachable!("named fields stay named")
            };
            item.fields = named;
            (kept || elision.kept_any)
                .then(|| mark_elisions(&unparse(syn::Item::Union(item)), elision.marker))
        }
        syn::Item::Const(item) if kept => {
            let mut item = item.clone();
            item.expr = Box::new(elided_value());
            Some(mark_elisions(&unparse(syn::Item::Const(item)), ""))
        }
        syn::Item::Static(item) if kept => {
            let mut item = item.clone();
            item.expr = Box::new(elided_value());
            Some(mark_elisions(&unparse(syn::Item::Static(item)), ""))
        }
        item if kept => Some(unparse(item.clone())),
        _ => None,
    }
}

/// A trait impl becomes a header line on the surface of the type it is
/// written for; an inherent impl keeps the items belonging to `surface`, and
/// is dropped if it has none.
fn render_impl(item: &syn::ItemImpl, module: &Module, surface: Surface) -> Option<String> {
    let mut item = item.clone();
    // An impl has no visibility of its own; it is as reachable as the type
    // and trait it names.
    let exported = module.exported
        && type_is_exported(&item.self_ty, module)
        && item
            .trait_
            .as_ref()
            .is_none_or(|(path, _)| path_is_exported(path, module));
    if item.trait_.is_some() {
        if exported != (surface == Surface::Public) {
            return None;
        }
        item.items.clear();
        let header = unparse(syn::Item::Impl(item));
        let header = header.trim_end_matches("{}").trim_end();
        // A where clause puts the empty body on a line of its own.
        return Some(header.trim_end_matches(',').to_owned());
    }
    item.items.retain(|item| {
        let vis = match item {
            syn::ImplItem::Const(item) => &item.vis,
            syn::ImplItem::Fn(item) => &item.vis,
            syn::ImplItem::Type(item) => &item.vis,
            _ => return false,
        };
        keep(vis, exported, surface)
    });
    if item.items.is_empty() {
        return None;
    }
    for item in &mut item.items {
        match item {
            syn::ImplItem::Fn(item) => item.block = empty_block(),
            syn::ImplItem::Const(item) => item.expr = elided_value(),
            _ => {}
        }
    }
    Some(mark_elisions(
        &signatures_only(&unparse(syn::Item::Impl(item))),
        "",
    ))
}

/// Whether something with visibility `vis`, declared in a context that is
/// itself exported or not, belongs on `surface`.
fn keep(vis: &syn::Visibility, in_exported: bool, surface: Surface) -> bool {
    let exported = in_exported && is_public(vis);
    match surface {
        Surface::Public => exported,
        Surface::Internal => visibility(vis).is_some() && !exported,
    }
}

/// Whether a type named by an impl is exported. Types from elsewhere are
/// assumed to be - only a type declared privately in this very module can be
/// ruled out without resolving paths.
fn type_is_exported(ty: &syn::Type, module: &Module) -> bool {
    let syn::Type::Path(ty) = ty else {
        return true;
    };
    path_is_exported(&ty.path, module)
}

fn path_is_exported(path: &syn::Path, module: &Module) -> bool {
    let Some(name) = path.segments.last() else {
        return true;
    };
    let name = name.ident.to_string();
    module
        .items
        .iter()
        .filter(|item| item_name(item).is_some_and(|declared| declared == name))
        .all(|item| item_visibility(item).is_some_and(is_public))
}

fn item_name(item: &syn::Item) -> Option<String> {
    let ident = match item {
        syn::Item::Enum(item) => &item.ident,
        syn::Item::Struct(item) => &item.ident,
        syn::Item::Trait(item) => &item.ident,
        syn::Item::TraitAlias(item) => &item.ident,
        syn::Item::Type(item) => &item.ident,
        syn::Item::Union(item) => &item.ident,
        _ => return None,
    };
    Some(ident.to_string())
}

fn item_attrs(item: &syn::Item) -> Option<&[syn::Attribute]> {
    let attrs = match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => return None,
    };
    Some(attrs)
}

fn item_visibility(item: &syn::Item) -> Option<&syn::Visibility> {
    let vis = match item {
        syn::Item::Const(item) => &item.vis,
        syn::Item::Enum(item) => &item.vis,
        syn::Item::ExternCrate(item) => &item.vis,
        syn::Item::Fn(item) => &item.vis,
        syn::Item::Mod(item) => &item.vis,
        syn::Item::Static(item) => &item.vis,
        syn::Item::Struct(item) => &item.vis,
        syn::Item::Trait(item) => &item.vis,
        syn::Item::TraitAlias(item) => &item.vis,
        syn::Item::Type(item) => &item.vis,
        syn::Item::Union(item) => &item.vis,
        syn::Item::Use(item) => &item.vis,
        _ => return None,
    };
    Some(vis)
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// `Some("pub")`, `Some("pub(crate)")` and so on for anything visible outside
/// its own module; `None` for a private item.
fn visibility(vis: &syn::Visibility) -> Option<String> {
    match vis {
        syn::Visibility::Public(_) => Some("pub".to_owned()),
        syn::Visibility::Restricted(restricted) => {
            let path = restricted
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            // `pub(self)` is private; anything else escapes the module.
            if path == "self" {
                return None;
            }
            let in_token = if restricted.in_token.is_some() {
                "in "
            } else {
                ""
            };
            Some(format!("pub({in_token}{path})"))
        }
        syn::Visibility::Inherited => None,
    }
}

fn is_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.meta {
        syn::Meta::List(list) => {
            list.path.is_ident("cfg") && list.tokens.to_string().contains("test")
        }
        _ => false,
    })
}

fn empty_block() -> syn::Block {
    syn::Block {
        brace_token: Default::default(),
        stmts: Vec::new(),
    }
}

fn elided_value() -> syn::Expr {
    syn::parse_str(ELIDED_VALUE).unwrap()
}

struct Elision {
    /// Whether any field belongs on this surface.
    kept_any: bool,
    /// What the fields left out stand in as.
    marker: &'static str,
}

/// Drops the fields that don't belong on `surface`, leaving a marker field in
/// their place. `None` if the type has no fields at all to split.
fn elide_fields(fields: &mut syn::Fields, exported: bool, surface: Surface) -> Option<Elision> {
    let mut elided_exported = false;
    let mut keep_field = |field: &syn::Field| {
        let kept = keep(&field.vis, exported, surface);
        elided_exported |= !kept && exported && is_public(&field.vis);
        kept
    };
    let (before, after) = match fields {
        syn::Fields::Named(named) => {
            let before = named.named.len();
            named.named = named
                .named
                .iter()
                .filter(|f| keep_field(f))
                .cloned()
                .collect();
            (before, named.named.len())
        }
        syn::Fields::Unnamed(unnamed) => {
            let before = unnamed.unnamed.len();
            unnamed.unnamed = unnamed
                .unnamed
                .iter()
                .filter(|f| keep_field(f))
                .cloned()
                .collect();
            (before, unnamed.unnamed.len())
        }
        syn::Fields::Unit => (0, 0),
    };
    // From outside the crate every field left out is simply private; inside
    // it, the ones left out are the exported fields listed in the other file.
    let marker = match surface == Surface::Internal && elided_exported {
        true => "/* other fields */",
        false => "/* private fields */",
    };
    if before != after {
        match fields {
            syn::Fields::Named(named) => named.named.push(
                syn::Field::parse_named
                    .parse_str(&format!("{ELIDED_FIELDS}: ()"))
                    .unwrap(),
            ),
            syn::Fields::Unnamed(unnamed) => unnamed
                .unnamed
                .push(syn::Field::parse_unnamed.parse_str(ELIDED_FIELDS).unwrap()),
            syn::Fields::Unit => unreachable!("a unit type has no fields to elide"),
        }
    }
    Some(Elision {
        kept_any: after > 0,
        marker,
    })
}

fn unparse(item: syn::Item) -> String {
    let file = syn::File {
        shebang: None,
        frontmatter: None,
        attrs: Vec::new(),
        items: vec![item],
    };
    prettyplease::unparse(&file).trim_end().to_owned()
}

/// Replaces the empty bodies left behind by [`empty_block`] with `;`.
fn signatures_only(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.trim() == "{}" {
            // A where clause puts the body on a line of its own.
            let signature = lines.last_mut().expect("a body follows a signature");
            signature.truncate(signature.trim_end_matches(',').len());
            signature.push(';');
        } else if let Some(signature) = line.strip_suffix(" {}") {
            lines.push(format!("{signature};"));
        } else {
            lines.push(line.to_owned());
        }
    }
    lines.join("\n")
}

/// Turns the markers left behind by [`elided_value`] and [`elide_fields`]
/// into the comments they stand for.
fn mark_elisions(text: &str, fields_marker: &str) -> String {
    text.replace(&format!("{ELIDED_FIELDS}: (),"), fields_marker)
        .replace(ELIDED_FIELDS, fields_marker)
        .replace(ELIDED_VALUE, "...")
}

const EXAMPLE: &str = r#"
    //! Module docs.
    /// Item docs.
    pub struct Named {
        pub exported: u8,
        pub(crate) shared: u8,
        private: u8,
    }
    pub struct Tuple(pub u8, u16);
    pub(crate) struct AllPrivate {
        private: u8,
    }
    pub(self) fn private_by_path() {}
    fn private() {}
    pub(in crate::page) fn restricted() -> u8 {}
    pub static COUNT: usize = 1 + 1;
    pub type Alias = Vec<u8>;
    struct Hidden;
    impl Trait for Named {
        fn method(&self) {}
    }
    impl Trait for Hidden {
        fn method(&self) {}
    }
    impl Named {
        pub fn exported(&self) {}
        pub(crate) fn shared(&self) {}
        fn private(&self) {}
    }
    impl AllPrivate {
        fn private(&self) {}
    }
    #[macro_export]
    macro_rules! exported { () => {} }
    macro_rules! internal { () => {} }
    #[cfg(test)]
    pub fn test_only() {}
    #[cfg(unix)]
    pub fn on_unix() {}
    pub fn with_where<T>(x: T) -> T
    where
        T: Clone + Send + Sync + std::fmt::Debug + Default + Ord,
    {}
"#;

/// Covers the shapes that the crates themselves don't currently have.
#[test]
fn exported_surface_covers_every_shape() {
    let module = Module {
        path: "example".to_owned(),
        declaration: "pub mod".to_owned(),
        exported: true,
        items: parse_source(EXAMPLE).unwrap(),
    };
    assert_eq!(
        render_module(&module, Surface::Public),
        r#"
## pub mod example

pub struct Named {
    pub exported: u8,
    /* private fields */
}

pub struct Tuple(pub u8, /* private fields */);
pub static COUNT: usize = ...;
pub type Alias = Vec<u8>;

impl Trait for Named

impl Named {
    pub fn exported(&self);
}

#[macro_export]
macro_rules! exported { /* ... */ }

#[cfg(unix)]
pub fn on_unix();

pub fn with_where<T>(x: T) -> T
where
    T: Clone + Send + Sync + std::fmt::Debug + Default + Ord;
"#
    );
}

#[test]
fn internal_surface_covers_every_shape() {
    let module = Module {
        path: "example".to_owned(),
        declaration: "pub mod".to_owned(),
        exported: true,
        items: parse_source(EXAMPLE).unwrap(),
    };
    assert_eq!(
        render_module(&module, Surface::Internal),
        r#"
## pub mod example

pub struct Named {
    pub(crate) shared: u8,
    /* other fields */
}

pub(crate) struct AllPrivate {
    /* private fields */
}

pub(in crate::page) fn restricted() -> u8;

impl Trait for Hidden

impl Named {
    pub(crate) fn shared(&self);
}
"#
    );
}

/// Inside a module that isn't exported, even `pub` items are internal.
#[test]
fn unexported_module_has_no_public_surface() {
    let source = "
        pub struct Named {
            pub field: u8,
        }
        impl Named {
            pub fn method(&self) {}
        }
    ";
    let module = Module {
        path: "example".to_owned(),
        declaration: "mod".to_owned(),
        exported: false,
        items: parse_source(source).unwrap(),
    };
    assert_eq!(
        render_module(&module, Surface::Public),
        "\n## mod example\n"
    );
    assert_eq!(
        render_module(&module, Surface::Internal),
        r#"
## mod example

pub struct Named {
    pub field: u8,
}

impl Named {
    pub fn method(&self);
}
"#
    );
}
