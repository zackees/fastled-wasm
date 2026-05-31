#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_span;

use rustc_errors::DiagDecorator;
use rustc_hir::{def::Res, AmbigArg, Expr, ExprKind, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_span::{symbol::Symbol, FileName, RemapPathScopeComponents};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Bans `std::path::PathBuf` outside the explicit legacy allowlist.
    ///
    /// ### Why is this bad?
    ///
    /// Raw `PathBuf` values do not carry fastled-cli's normalization
    /// invariant. Windows long-path canonicalize() output (`\\?\C:\...`)
    /// has crashed external tools we hand paths to (meson, Python's
    /// `open()`, emcc) — see issue #114. Use
    /// `fastled_cli::path::NormalizedPath` at every boundary instead.
    ///
    /// ### Known problems
    ///
    /// The workspace still has legacy `PathBuf` call sites. Those files
    /// are temporarily allowlisted; remove entries from
    /// `src/allowlist.txt` as they migrate.
    ///
    /// ### Example
    ///
    /// ```rust
    /// use std::path::PathBuf;
    /// let path = PathBuf::from("src/foo.rs");
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// use fastled_cli::path::NormalizedPath;
    /// let path = NormalizedPath::new("src/foo.rs");
    /// ```
    pub BAN_STD_PATHBUF,
    Deny,
    "ban std::path::PathBuf outside the legacy allowlist"
}

const PATHBUF_DEF_PATH: &[&str] = &["std", "path", "PathBuf"];
const ALLOWLIST: &str = include_str!("allowlist.txt");

impl<'tcx> LateLintPass<'tcx> for BanStdPathbuf {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, ty: &'tcx Ty<'tcx, AmbigArg>) {
        if is_allowlisted(cx, ty.span) {
            return;
        }

        if let TyKind::Path(qpath) = ty.kind {
            let res = cx.qpath_res(&qpath, ty.hir_id);
            if res_is_pathbuf(cx, res) {
                emit_lint(cx, ty.span);
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if is_allowlisted(cx, expr.span) {
            return;
        }

        if let ExprKind::Path(qpath) = expr.kind {
            let res = cx.qpath_res(&qpath, expr.hir_id);
            if res_is_pathbuf_assoc(cx, res) {
                emit_lint(cx, expr.span);
            }
        }
    }
}

fn emit_lint(cx: &LateContext<'_>, span: rustc_span::Span) {
    cx.opt_span_lint(
        BAN_STD_PATHBUF,
        Some(span),
        DiagDecorator(|diag| {
            diag.primary_message(
                "use fastled_cli::path::NormalizedPath instead of std::path::PathBuf",
            );
        }),
    );
}

fn is_allowlisted(cx: &LateContext<'_>, span: rustc_span::Span) -> bool {
    let filename = match cx.sess().source_map().span_to_filename(span) {
        FileName::Real(real_filename) => real_filename
            .local_path()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                real_filename
                    .path(RemapPathScopeComponents::DIAGNOSTICS)
                    .to_string_lossy()
                    .into_owned()
            }),
        filename => filename
            .display(RemapPathScopeComponents::DIAGNOSTICS)
            .to_string(),
    };
    let normalized = filename.replace('\\', "/");
    ALLOWLIST
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .any(|allowed| normalized.ends_with(allowed))
}

fn res_is_pathbuf(cx: &LateContext<'_>, res: Res) -> bool {
    match res {
        Res::Def(_, def_id) => def_path_starts_with(cx, def_id, PATHBUF_DEF_PATH),
        _ => false,
    }
}

fn res_is_pathbuf_assoc(cx: &LateContext<'_>, res: Res) -> bool {
    match res {
        Res::Def(_, def_id) => {
            let def_path = cx.get_def_path(def_id);
            def_path.len() > PATHBUF_DEF_PATH.len()
                && def_path
                    .iter()
                    .take(PATHBUF_DEF_PATH.len())
                    .zip(PATHBUF_DEF_PATH.iter())
                    .all(|(actual, expected)| *actual == Symbol::intern(expected))
        }
        _ => false,
    }
}

fn def_path_starts_with(
    cx: &LateContext<'_>,
    def_id: rustc_hir::def_id::DefId,
    prefix: &[&str],
) -> bool {
    let def_path = cx.get_def_path(def_id);
    def_path.len() >= prefix.len()
        && def_path
            .iter()
            .take(prefix.len())
            .zip(prefix.iter())
            .all(|(actual, expected)| *actual == Symbol::intern(expected))
}
