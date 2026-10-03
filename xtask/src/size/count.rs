//! Counting the lines of one source file that are not test-only.

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ImplItem, Item, ItemMod, Lit, Meta, Token, TraitItem};

/// An out-of-line module declaration (`mod name;`).
#[derive(Debug, PartialEq, Eq)]
pub struct ModDecl {
    /// Inline modules enclosing the declaration, outermost first.
    pub parents: Vec<String>,
    /// The module name.
    pub name: String,
    /// The value of a `#[path = "..."]` attribute, if any.
    pub path: Option<String>,
    /// Whether the declaration only exists under `cfg(test)`.
    pub test_only: bool,
}

/// What [`analyze`] learned about a file.
#[derive(Debug)]
pub struct Analysis {
    /// Lines outside `#[cfg(test)]` items.
    pub counted: usize,
    /// Out-of-line modules declared in the file.
    pub modules: Vec<ModDecl>,
}

/// Parses `source` and counts its lines, minus the lines spanned by items
/// annotated `#[cfg(test)]` (including their attributes and doc comments).
pub fn analyze(source: &str) -> syn::Result<Analysis> {
    let file = syn::parse_file(source)?;
    let whole_file = is_test_only(&file.attrs);
    let mut collector = Collector {
        test_depth: usize::from(whole_file),
        ..Collector::default()
    };
    collector.visit_file(&file);
    let counted = if whole_file {
        0
    } else {
        source
            .lines()
            .count()
            .saturating_sub(merged_len(&mut collector.ranges))
    };
    Ok(Analysis {
        counted,
        modules: collector.modules,
    })
}

#[derive(Default)]
struct Collector {
    ranges: Vec<(usize, usize)>,
    parents: Vec<String>,
    modules: Vec<ModDecl>,
    test_depth: usize,
}

impl Collector {
    fn within(&mut self, attrs: &[Attribute], span: Span, visit: impl FnOnce(&mut Self)) {
        let test = is_test_only(attrs);
        if test {
            if self.test_depth == 0 {
                self.ranges.push((span.start().line, span.end().line));
            }
            self.test_depth += 1;
        }
        visit(self);
        if test {
            self.test_depth -= 1;
        }
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, item: &'ast Item) {
        self.within(item_attrs(item), item.span(), |this| {
            visit::visit_item(this, item);
        });
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        self.within(impl_item_attrs(item), item.span(), |this| {
            visit::visit_impl_item(this, item);
        });
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        self.within(trait_item_attrs(item), item.span(), |this| {
            visit::visit_trait_item(this, item);
        });
    }

    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        let name = module.ident.to_string();
        if module.content.is_none() {
            self.modules.push(ModDecl {
                parents: self.parents.clone(),
                name,
                path: path_attr(&module.attrs),
                test_only: self.test_depth > 0,
            });
        } else {
            self.parents.push(name);
            visit::visit_item_mod(self, module);
            self.parents.pop();
        }
    }
}

/// Total number of lines covered by the inclusive `ranges`, counting overlaps once.
fn merged_len(ranges: &mut [(usize, usize)]) -> usize {
    ranges.sort_unstable();
    let mut total = 0;
    let mut covered_to = 0;
    for &(start, end) in ranges.iter() {
        let start = start.max(covered_to + 1);
        if end >= start {
            total += end - start + 1;
            covered_to = end;
        }
    }
    total
}

fn is_test_only(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .is_ok_and(|meta| requires_test(&meta))
    })
}

/// Whether a `cfg` predicate can only hold when compiling tests.
fn requires_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            .is_ok_and(|args| args.iter().any(requires_test)),
        Meta::List(_) | Meta::NameValue(_) => false,
    }
}

fn path_attr(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| match &attr.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            Expr::Lit(expr) => match &expr.lit {
                Lit::Str(lit) => Some(lit.value()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(i) => &i.attrs,
        ImplItem::Fn(i) => &i.attrs,
        ImplItem::Type(i) => &i.attrs,
        ImplItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(i) => &i.attrs,
        TraitItem::Fn(i) => &i.attrs,
        TraitItem::Type(i) => &i.attrs,
        TraitItem::Macro(i) => &i.attrs,
        _ => &[],
    }
}

#[cfg(test)]
mod tests;
