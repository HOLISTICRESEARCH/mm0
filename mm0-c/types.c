#include <stdint.h>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

#define ALIGNED(n) __attribute__((aligned(1)))

// Each sort has one byte associated to it, which
// contains flags for the sort modifiers.
// The high four bits are unused.
#define SORT_PURE     1
#define SORT_STRICT   2
#define SORT_PROVABLE 4
#define SORT_FREE     8

#define MM0B_MAGIC (u32)0x42304D4D // = "MM0B"
#define MM0B_VERSION (u8)1
#define MAX_SORTS 128

typedef struct {
  u32 magic;         // = MM0B_MAGIC
  u8 version;        // = MM0B_VERSION

  u8 num_sorts;      // number of declared sorts ( <= MAX_SORTS )
  u16 reserved;
  u32 num_terms;     // number of terms and defs
  u32 num_thms;      // number of axioms and theorems

  /* term* */ u32 p_terms;  // pointer to start of term table
  /* thm*  */ u32 p_thms;   // pointer to start of theorem table
  u32 p_proof;              // pointer to start of proof section
  u64 p_index;              // pointer to start of index, or 0

  // The list of all sorts. The number of sorts is
  // limited to 128 because of the data layout.
  // So don't monomorphize too much.
  u8 sorts[MAX_SORTS];
} header;

// The term arguments table is composed of 64 bit values, where:
// * the low 56 bits (7 bytes) are used for encoding the list of bound variables
//   this variable depends on. bit i is 1 if this is a regular var and
//   depends on the ith *bound* variable
// * Bits 57-62 (the 7 low bits of the high byte) encode the sort of the expression
// * Bit 63 (the MSB / sign bit) is 1 if this is a bound var
#define TYPE_DEPS_MASK (((u64)1<<56)-1)
#define TYPE_BOUND_MASK ((u64)1<<63)
#define TYPE_SORT(type) (((type) >> 56) & 0x7F)

// An entry in the term table (8 byte aligned)
typedef struct {
  u16 num_args;          // number of arguments
  u8 sort;               // sort of the return value, 1 in high bit means this
                         // is a definition
  u8 reserved;
  /* u64* */ u32 p_args; // pointer to list of binders
  // The list of binders has n+1 elements, with the last element being
  // the return type, followed by a CMD_END-terminated unification command list
  // for definitions.
} term;

// An entry in the theorem table (4 byte aligned)
typedef struct {
  u16 num_args;           // number of arguments (expression assumptions)
  u16 reserved;
  /* u64* */ u32 p_args; // pointer to list of binders
  // The list of binders has n elements, followed by a CMD_END-terminated
  // unification command list.
} thm;

#define INDEX_KIND_TERM      (u8)0x01 // This is a term
#define INDEX_KIND_AXIOM     (u8)0x02 // This is an axiom
#define INDEX_KIND_VAR       (u8)0x03 // This is a variable
#define INDEX_KIND_SORT      (u8)0x04 // This is a sort
#define INDEX_KIND_DEF       (u8)0x05 // This is a def
#define INDEX_KIND_THM       (u8)0x06 // This is a theorem
#define INDEX_KIND_LOCAL_DEF (u8)0x0D // This is a def that does not appear in the spec
#define INDEX_KIND_LOCAL_THM (u8)0x0E // This is a theorem that does not appear in the spec

// The index contains information not needed for the verifier but
// helpful for display and debugging (like string representations of the
// constants).
typedef struct {
  u64 left;                  // pointer to left subchild (for binary searching by strings)
  u64 right;                 // pointer to right subchild
  u8 kind;                   // sort, term, thm, var
  u8 padding[3];
  u32 ix;                    // Index of the object in the relevant table
  u32 row, col;              // For locating in the spec file
  u64 proof;                 // pointer to the command that declares this item
  char value[];              // zero-terminated char* buffer
} index;

// A command is a variable length instruction that forms the bulk of the proof
// file. The commands are processed by a stack machine.
//
// The high two bits denote the data size. The value 0 can be encoded in 0
// bytes, and unsigned integers of 8, 16, 32 bytes are encoded here. This is
// sufficient to determine the length of a command.

#define CMD_DATA(opcode) ((opcode) & 0xC0)
#define CMD_DATA_0  0x00
#define CMD_DATA_8  0x40
#define CMD_DATA_16 0x80
#define CMD_DATA_32 0xC0

// Bit 5 is unused. The opcode determines the data layout of following fields.
// In each data layout, the fields are marked as:
//
//   (input) if they are new information for the verifier
//   (output) if they are determinable from information the verifier already
//            knows, and the verifier is responsible for checking that the data
//            is correct
//   (unverified) if the information should be well formed but the verifier
//                can't check it. This info is ignored in core verification

// Opcodes 0 - 15 are command keywords; they start a new statement (axiom, def, theorem, etc).
// Except the END instruction, each is followed by a pointer to the next command keyword.

// End: A null statement, the end of a command sequence. Data = 0.
// Although the command does not reserve space after it, no command can appear
// within 12 bytes (the longest command structure) of the end of the file,
// to allow for preloading. So if CMD_END is the last thing in the file there
// should be another 19 bytes padding.
#define CMD_MAX_SIZE 12
#define CMD_END 0x00

// The statement commands are the same as the corresponding INDEX_KINDs, except
// that there is no CMD_STMT_VAR, and CMD_STMT_TERM is the same as CMD_STMT_DEF
#define CMD_STMT_TERM      0x05
#define CMD_STMT_AXIOM     0x02
#define CMD_STMT_SORT      0x04
#define CMD_STMT_DEF       0x05
#define CMD_STMT_THM       0x06
#define CMD_STMT_LOCAL_DEF 0x0D
#define CMD_STMT_LOCAL_THM 0x0E

// is CMD_STMT_THM or CMD_STMT_LOCAL_THM
#define IS_CMD_STMT_THM(opcode) (((opcode) & 0xF7) == CMD_STMT_THM)

// All commands are byte aligned, and have a forward reference to the
// next command.
typedef struct ALIGNED(1) {
  u8 cmd;           // statement command
  u32 next;         // the number of bytes to the next statement command (output)
  u8 proof[];       // Proof commands begin here
} cmd_stmt;

typedef struct ALIGNED(1) {
  u8 cmd;
} cmd;

// The length of the data field depends on the high bits of the command
typedef struct ALIGNED(1) { u8 cmd; u8 data; } cmd8;
typedef struct ALIGNED(1) { u8 cmd; u16 data; } cmd16;
typedef struct ALIGNED(1) { u8 cmd; u32 data; } cmd32;

// Term: Pop n expressions from the stack (n is determined from the term ID),
// and push a term applied to these expressions. (The n elements are popped
// as a group so that they end up in the same order as they were pushed.)
// Uses data = termid
#define CMD_PROOF_TERM 0x10

// TermSave: Same as Term, but also adds the resulting expression to the heap.
// Uses data = termid
#define CMD_PROOF_TERM_SAVE 0x11

// Ref: Push a variable / hypothesis or previously constructed
// expression / theorem.
// Uses data = heapid, a reference to the heap element.
#define CMD_PROOF_REF 0x12

// Dummy: Push a new variable on the stack, and add it to the heap.
// Uses data = sortid
#define CMD_PROOF_DUMMY 0x13

// Thm: Pop an expression from the stack, pop n subproofs, pop m expressions
// (m and n are determined from the theorem) and check that the substitution
// of the expressions into the conclusion of the theorem is the given
// expression, and the hyps match their substitutions as well.
// Uses data = thmid
#define CMD_PROOF_THM 0x14

// ThmSave: Same as Thm, but also adds the resulting subproof to the heap.
// Uses data = thmid
#define CMD_PROOF_THM_SAVE 0x15

// Hyp: Pop an expression from the stack, and ensure that the unifier for
// the current theorem declares this hypothesis correctly.
// Uses data = 0
#define CMD_PROOF_HYP 0x16

// Conv: Pop a proof of e2, pop an expression e1, push a proof of e1, push
// a convertibility obligation e1 =?= e2.
// Uses data = 0
#define CMD_PROOF_CONV 0x17

// Refl: Pop a convertibility obligation e =?= e. The two sides should be
// references to the same heap element.
// Uses data = 0
#define CMD_PROOF_REFL 0x18

// Symm: Pop a convertibility obligation e1 =?= e2, push a convertibility
// obligation e2 =?= e1.
// Uses data = 0
#define CMD_PROOF_SYMM 0x19

// Cong: Pop a convertibility obligation t e1 ... en =?= t e1' ... en',
// push e1 =?= e1', ..., push en =?= en'.
// Uses data = 0
#define CMD_PROOF_CONG 0x1A

// Unfold: Pop a convertibility obligation t e1 ... en =?= e', where t is a
// definition, and execute the unifier for t, which will pop an additional
// m+1 expressions from the stack, where m is the number of heap elements in
// the definition of t. The last expression popped, e, is the result of the
// unfolding; push e =?= e' to the stack.
// Uses data = 0
#define CMD_PROOF_UNFOLD 0x1B

// ConvCut: Pop an expression e2, pop an expression e1, push a proof of
// e1 = e2, push a convertibility obligation e1 =?= e2.
// Uses data = 0
#define CMD_PROOF_CONV_CUT 0x1C

// ConvRef: Pop a convertibility obligation e1 =?= e2, where e1 = e2 is the
// referenced heap element.
// Uses data = heapid
#define CMD_PROOF_CONV_REF 0x1D

// Unify commands are used in definitions and theorem statements.
// They are consumed when a definition is unfolded, or when a theorem is
// applied, and interact with both the main stack and a separate unification
// stack, which stores expressions that will be matched relative to an input
// substitution. The substitution is stored on a separate unify heap, which
// may be extended during unification via Save commands.

// Term: Pop an expression from the unify stack, ensure that the head
// of the expression is the given term ID, and push the n arguments to the
// unify stack.
// Uses data = termid
#define CMD_UNIFY_TERM 0x30

// TermSave: Same as Term, but also puts the expression popped from the
// unify stack on the substitution heap.
// Uses data = termid
#define CMD_UNIFY_TERM_SAVE 0x31

// Ref: Pop an expression from the unify stack, and ensure it is equal
// to the referenced element on the substitution heap.
// Uses data = heapid, a reference to the substitution heap.
#define CMD_UNIFY_REF 0x32

// Dummy: (Only in definitions) Pop an expression from the unify stack,
// check that it is a bound variable with the specified sort, and
// put it on the substitution heap.
// Uses data = sortid
#define CMD_UNIFY_DUMMY 0x33

// Thm: (Only in theorem statements) Pop an expression e from the main stack,
// and record it for later; we will push a proof of e at the end of
// unification. Push e to the unify stack.
// Uses data = 0
#define CMD_UNIFY_THM 0x34

// Hyp: (Only in theorem statements) Pop a proof of e from the main stack,
// and push e to the unify stack.
// Uses data = 0
#define CMD_UNIFY_HYP 0x36
