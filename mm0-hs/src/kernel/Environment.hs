module Environment where

import Data.Default
import qualified Data.Text as T
import qualified Data.Map.Strict as M
import qualified Data.Sequence as Q

type Ident = T.Text
type Sort = Ident
type TermName = Ident
type ThmName = Ident
type VarName = Ident
type Token = Ident

data DepType = DepType {
  dSort :: Sort,
  dDeps :: [VarName] } deriving (Eq)

instance Show DepType where
  showsPrec _ (DepType t ts) r =
    T.unpack t ++ foldr (\t' r' -> ' ' : T.unpack t' ++ r') r ts

data PBinder = PBound VarName Ident | PReg VarName DepType

instance Show PBinder where
  showsPrec _ (PBound v t) r = T.unpack v ++ ": " ++ T.unpack t ++ r
  showsPrec n (PReg v t) r = T.unpack v ++ ": " ++ showsPrec n t r

binderName :: PBinder -> VarName
binderName (PBound v _) = v
binderName (PReg v _) = v

binderType :: PBinder -> DepType
binderType (PBound _ t) = (DepType t [])
binderType (PReg _ ty) = ty

binderBound :: PBinder -> Bool
binderBound (PBound _ _) = True
binderBound (PReg _ _) = False

data SExpr = SVar VarName | App TermName [SExpr] deriving (Eq)

instance Show SExpr where
  showsPrec _ (SVar v) r = T.unpack v ++ r
  showsPrec _ (App v []) r = T.unpack v ++ r
  showsPrec n (App v vs) r =
    let f r1 = T.unpack v ++ foldr (\e r2 -> ' ' : showsPrec 1 e r2) r1 vs in
    if n == 0 then f r else '(' : f (')' : r)

data Decl =
    DTerm
      [PBinder]  -- bound variables, args
      DepType    -- return type
  | DAxiom
      [PBinder]  -- bound variables, args
      [SExpr]    -- hypotheses
      SExpr      -- conclusion
  | DDef
      [PBinder]  -- bound variables, args
      DepType    -- return type
      (Maybe (
        M.Map Ident Ident, -- dummy vars
        SExpr))            -- definition
  deriving (Show)

data SortData = SortData {
  sPure :: Bool,
  sStrict :: Bool,
  sProvable :: Bool,
  sFree :: Bool }
  deriving (Show)

data IOKind = IOKString Bool SExpr deriving (Show)

data Spec =
    SSort Ident SortData
  | SDecl Ident Decl
  | SThm {
      tName :: Ident,
      tArgs :: [PBinder],
      tHyps :: [SExpr],
      tReturn :: SExpr }
  | SInout IOKind
  deriving (Show)

data Environment = Environment {
  eSorts :: M.Map Ident SortData,
  eDecls :: M.Map Ident Decl,
  eSpec :: Q.Seq Spec }
  deriving (Show)

instance Default Environment where
  def = Environment def def def

getTerm :: Environment -> TermName -> Maybe ([PBinder], DepType)
getTerm e v = eDecls e M.!? v >>= go where
  go (DTerm args r) = Just (args, r)
  go (DDef args r _) = Just (args, r)
  go (DAxiom _ _ _) = Nothing

getArity :: Environment -> TermName -> Maybe Int
getArity e v = length . fst <$> getTerm e v
