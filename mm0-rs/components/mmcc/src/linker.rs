//! Handles layout of functions, globals, constants in the overall program.

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::hash::Hash;

use crate::types::global::{self, TyKind, ExprKind};
use crate::types::entity::{ConstTc, Entity, ProcTc};
use crate::types::mir::{ConstKind, Constant, Place, Proc, RValue, Terminator, Ty, Visitor};
use crate::types::vcode::{GlobalId, ProcId, ConstRef};
use crate::types::{IdxVec, Size, Spanned};
use crate::{Idx, Symbol};

type GenericCall = (Symbol, Box<[Ty]>);

type ConstVal = (u32, ConstRef);

#[derive(Default)]
pub(crate) struct ConstData {
  map: HashMap<Symbol, ConstVal>,
  rodata: Vec<u8>,
}

impl std::ops::Index<Symbol> for ConstData {
  type Output = ConstVal;
  fn index(&self, s: Symbol) -> &Self::Output { &self.map[&s] }
}

impl ConstData {
  pub(crate) fn load_mem(&self, sz: Size, addr: u32) -> u64 {
    let len = sz.bytes().expect("size Inf not allowed").into();
    let mut buf = [0; 8];
    buf[..len].copy_from_slice(&self.rodata[addr.try_into().expect("overflow")..][..len]);
    u64::from_le_bytes(buf)
  }

  pub(crate) fn value(&self, (sz, c): ConstVal) -> u64 {
    match c {
      ConstRef::Value(val) => val,
      ConstRef::Ptr(addr) => self.load_mem(Size::from_u64(sz.into()), addr)
    }
  }

  fn here(&self) -> u32 {
    self.rodata.len().try_into().expect("overflow")
  }

  fn alloc(&mut self, buf: &[u8]) -> u32 {
    let addr = self.here();
    self.rodata.extend_from_slice(buf);
    addr
  }
}

struct Collector<'a> {
  names: &'a HashMap<Symbol, Entity>,
  mir: &'a HashMap<Symbol, Proc>,
  implications: HashMap<Symbol, Option<HashSet<GenericCall>>>,
  funcs: (HashMap<Symbol, ProcId>, IdxVec<ProcId, Symbol>),
  globals: (HashMap<Symbol, GlobalId>, IdxVec<GlobalId, u32>),
  consts: ConstData,
}

impl<'a> Collector<'a> {
  fn new(names: &'a HashMap<Symbol, Entity>, mir: &'a HashMap<Symbol, Proc>) -> Self {
    Self {
      names,
      mir,
      implications: Default::default(),
      funcs: Default::default(),
      globals: Default::default(),
      consts: Default::default(),
    }
  }

  fn collect_generics(&mut self, f: Symbol, args: &[Ty], calls: &HashSet<GenericCall>) {
    for &(g, ref tys) in calls {
      let args: Box<[_]> = tys.iter().map(|ty| ty.subst(args)).collect();
      self.collect_func(g, &args);
    }
  }

  fn collect_func(&mut self, f: Symbol, args: &[Ty]) -> ProcId {
    if !args.is_empty() {
      unimplemented!("functions with type args")
    }
    if let Some(&id) = self.funcs.0.get(&f) { return id }
    let id = self.funcs.1.push(f);
    self.funcs.0.insert(f, id);
    if let Some(imps) = self.implications.get_mut(&f) {
      let calls = imps.take().expect("cycle in collector?");
      self.collect_generics(f, args, &calls);
      self.implications.insert(f, Some(calls));
    } else if let Some(proc) = self.mir.get(&f) {
      let mut calls = HashSet::new();
      for (_, bl) in proc.body.blocks() {
        struct ConstVisitor<'a, 'b>(&'b mut Collector<'a>);
        impl Visitor for ConstVisitor<'_, '_> {
          fn visit_place(&mut self, _: &Place) {}
          fn visit_constant(&mut self, c: &Constant) {
            if let ConstKind::Const(s) = c.k { self.0.collect_const(s); }
          }
        }
        ConstVisitor(self).visit_basic_block(bl);
        if let Terminator::Call { f, tys, .. } = bl.terminator() {
          if tys.iter().any(|ty| ty.has_tyvar()) {
            calls.insert((*f, tys.clone()));
          } else {
            self.collect_func(*f, tys);
          }
        }
      }
      self.implications.insert(f, None);
      self.collect_generics(f, args, &calls);
      self.implications.insert(f, Some(calls));
    }
    id
  }

  fn alloc_const(&mut self, ty: &TyKind, e: &ExprKind) -> Option<(u32, u32)> {
    Some(match self.eval_const(ty, e)? {
      (sz, ConstRef::Ptr(addr)) => (sz, addr),
      (sz, ConstRef::Value(val)) =>
        (sz, self.consts.alloc(&val.to_le_bytes()[..sz.try_into().expect("overflow")]))
    })
  }

  fn eval_const(&mut self, ty: &TyKind, e: &ExprKind) -> Option<ConstVal> {
    match e {
      ExprKind::Unit => Some((0, ConstRef::Value(0))),
      &ExprKind::Const(s) => Some(self.collect_const(s)),
      &ExprKind::Bool(b) => Some((1, ConstRef::Value(b.into()))),
      ExprKind::Int(n) => if let TyKind::Int(ity) = *ty {
        Some((ity.size().bytes()?.into(), ConstRef::Value(ity.zero_extend_as_u64(n)?)))
      } else {
        None
      }
      ExprKind::List(es) => match ty {
        TyKind::Sn(_, ty) => self.eval_const(ty, &es[0]),
        TyKind::List(tys) => {
          let addr = self.consts.here();
          for (ty, e) in tys.iter().zip(&**es) { self.alloc_const(ty, e)?; }
          Some((self.consts.here() - addr, ConstRef::Ptr(addr)))
        }
        TyKind::Struct(tys) => {
          let addr = self.consts.here();
          for (arg, e) in tys.iter().zip(&**es) { self.alloc_const(arg.1.ty(), e)?; }
          Some((self.consts.here() - addr, ConstRef::Ptr(addr)))
        }
        _ => None
      },
      ExprKind::Array(es) => if let TyKind::Array(ty, _) = ty {
        let addr = self.consts.here();
        for e in &**es { self.alloc_const(ty, e)?; }
        Some((self.consts.here() - addr, ConstRef::Ptr(addr)))
      } else {
        None
      }
      ExprKind::Var(_) |
      ExprKind::Unop(_, _) |
      ExprKind::Binop(_, _, _) |
      ExprKind::Index(_, _) |
      ExprKind::Slice(_) |
      ExprKind::Proj(_, _) |
      ExprKind::UpdateIndex(_) |
      ExprKind::UpdateSlice(_) |
      ExprKind::UpdateProj(_, _, _) |
      ExprKind::Sizeof(_) |
      ExprKind::Ref(_) |
      ExprKind::Mm0(_) |
      ExprKind::Call { .. } |
      ExprKind::If { .. } |
      ExprKind::Error => None, // TODO: const eval
    }
  }

  fn collect_const(&mut self, c: Symbol) -> ConstVal {
    if let Some(&id) = self.consts.map.get(&c) { return id }
    let value = if_chain! {
      if let Some(Entity::Const(tc)) = self.names.get(&c);
      if let ConstTc::Checked { ref ty, ref whnf, .. } = tc.k;
      then { self.eval_const(ty, whnf) }
      else { None }
    }.expect("cannot resolve constant to an integer value");
    self.consts.map.insert(c, value);
    value
  }
}
