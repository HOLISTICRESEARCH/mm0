pub mod environment;

use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::path::PathBuf;
use std::collections::{HashMap};
use lsp_types::{Diagnostic, DiagnosticRelatedInformation};
pub use environment::Environment;
pub use crate::parser::{ErrorLevel, BoxError};
use crate::parser::{*, ast::*};
use crate::lined_string::*;

pub enum ElabErrorKind {
  Boxed(BoxError, Option<Vec<(FileSpan, BoxError)>>),
}
impl ElabErrorKind {
  pub fn msg(&self) -> String {
    match self {
      ElabErrorKind::Boxed(e, _) => format!("{}", e),
    }
  }

  pub fn to_related_info(&self, file: &LinedString) -> Option<Vec<DiagnosticRelatedInformation>> {
    match self {
      ElabErrorKind::Boxed(_, Some(info)) =>
        Some(info.iter().map(|(fs, e)| DiagnosticRelatedInformation {
          location: file.to_loc(fs),
          message: format!("{}", e),
        }).collect()),
      _ => None
    }
  }
}

impl From<BoxError> for ElabErrorKind {
  fn from(e: BoxError) -> ElabErrorKind { ElabErrorKind::Boxed(e, None) }
}

pub struct ElabError {
  pub pos: Span,
  pub level: ErrorLevel,
  pub kind: ElabErrorKind,
}
type Result<T> = std::result::Result<T, ElabError>;

impl ElabError {
  pub fn new(pos: impl Into<Span>, kind: ElabErrorKind) -> ElabError {
    ElabError { pos: pos.into(), level: ErrorLevel::Error, kind }
  }
  pub fn new_e(pos: impl Into<Span>, e: impl Into<BoxError>) -> ElabError {
    ElabError::new(pos, ElabErrorKind::Boxed(e.into(), None))
  }
  pub fn with_info(pos: impl Into<Span>, msg: BoxError, v: Vec<(FileSpan, BoxError)>) -> ElabError {
    ElabError::new(pos, ElabErrorKind::Boxed(msg, Some(v)))
  }

  pub fn to_diag(&self, file: &LinedString) -> Diagnostic {
    Diagnostic {
      range: file.to_range(self.pos),
      severity: Some(self.level.to_diag_severity()),
      code: None,
      source: Some("mm0-rs".to_owned()),
      message: self.kind.msg(),
      related_information: self.kind.to_related_info(file),
    }
  }
}

struct Elaborator<'a, T: FileServer + ?Sized> {
  ast: &'a AST,
  fs: &'a T,
  path: FileRef,
  errors: Vec<ElabError>,
  toks: HashMap<Span, Option<T::WaitToken>>,
  env: Environment,
}

impl<T: FileServer + ?Sized> Deref for Elaborator<'_, T> {
  type Target = Environment;
  fn deref(&self) -> &Environment { &self.env }
}
impl<T: FileServer + ?Sized> DerefMut for Elaborator<'_, T> {
  fn deref_mut(&mut self) -> &mut Environment { &mut self.env }
}

impl<'a, T: FileServer + ?Sized> Elaborator<'a, T> {
  fn new(ast: &'a AST, path: FileRef, fs: &'a T) -> Elaborator<'a, T> {
    Elaborator {
      ast, fs, path,
      errors: Vec::new(),
      toks: HashMap::new(),
      env: Environment::default(),
    }
  }

  fn span(&self, s: Span) -> &str { self.ast.span(s) }
  fn fspan(&self, span: Span) -> FileSpan { FileSpan {file: self.path.clone(), span} }
  fn report(&mut self, e: ElabError) { self.errors.push(e) }

  fn elaborate_decl(&mut self, d: &Decl) {
    match d.k {
      _ => self.report(ElabError::new_e(d.id, "unimplemented"))
    }
  }

  fn add_simple_nota(&mut self, n: &SimpleNota) {
    match n.k {
      _ => self.report(ElabError::new_e(n.id, "unimplemented"))
    }
  }

  fn elaborate_stmt(&mut self, stmt: &Stmt) {
    match &stmt.k {
      &StmtKind::Sort(sp, sd) => {
        let s = Arc::new(self.span(sp).to_owned());
        let fsp = self.fspan(sp);
        if let (_, Err(r)) = self.add_sort(s.clone(), fsp, sd) {
          self.report(ElabError::with_info(sp, r.msg.into(), vec![(r.other, r.othermsg.into())]));
        }
      }
      StmtKind::Decl(d) => self.elaborate_decl(d),
      StmtKind::Delimiter(Delimiter::Both(f)) => self.env.add_delimiters(f, f),
      StmtKind::Delimiter(Delimiter::LeftRight(ls, rs)) => self.env.add_delimiters(ls, rs),
      StmtKind::SimpleNota(n) => self.add_simple_nota(n),
      &StmtKind::Import(sp, _) => {
        if let Some(ref tok) = self.toks[&sp] {
          let env = self.fs.get_elab(tok);
          self.env.merge(&env, sp, &mut self.errors)
        }
      }
      _ => self.report(ElabError::new_e(stmt.span, "unimplemented"))
    }
  }
}

pub trait FileServer {
  type WaitToken: Clone;
  fn request_elab(&self, path: PathBuf, f: impl Fn(BoxError) -> ElabError) ->
    Result<(FileRef, Self::WaitToken)>;

  fn get_elab(&self, tok: &Self::WaitToken) -> Arc<Environment>;

  fn elaborate<'a>(&'a self, path: FileRef, ast: &'a AST,
      _old: Option<(usize, Vec<ElabError>, Arc<Environment>)>) ->
      (Vec<ElabError>, Environment, Vec<FileRef>) {
    let mut elab = Elaborator::new(ast, path, self);
    let mut deps: Vec<FileRef> = Vec::new();
    for (sp, f) in &ast.imports {
      match elab.path.path().join(f).canonicalize()
        .map_err(|e| ElabError::new_e(sp.clone(), e))
        .and_then(|p| self.request_elab(p, |e| ElabError::new_e(sp.clone(), e))) {
        Ok((buf, tok)) => { deps.push(buf); elab.toks.insert(sp.clone(), Some(tok)); }
        Err(e) => { elab.errors.push(e); elab.toks.insert(sp.clone(), None); }
      }
    }

    for s in ast.stmts.iter() { elab.elaborate_stmt(s) }
    // unimplemented!()
    (elab.errors, elab.env, deps)
  }
}
