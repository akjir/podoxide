# PodScript Language (PSL) Specification

**Version:** 1.0.0
**Status:** Normative
**Target Module:** `src/io/psl/`

> [!IMPORTANT]
> **AI Agent Mandate (Single Source of Truth):**
> This document defines the lexical and syntactic rules of the PodScript Language (`PSL`) used in `pods.conf` and `*.pod` files. Whenever this specification is modified, you MUST follow the strict **Spec-Driven TDD Workflow** in Section 8: update or add unit/integration tests FIRST, and modify the parser implementation in `src/io/psl/` SECOND.

## 1. Overview & Design Goals

The PodScript Language (`PSL`) is a minimal, deterministic, line-oriented, block-based configuration and recipe language designed for **Podoxide**. It combines the readability of CLI arguments with structured `{ ... }` blocks.

### Core Design Principles
1. **Strict Root Block Enforcement:** Every top-level statement in a PSL document MUST be a block. Standalone directives at the root level are forbidden.
2. **Unified Grammar (`P-UNIFIED-PARSER`):** Both `pods.conf` (application configuration) and `*.pod` (Podman recipes) share the exact same lexical scanner and structural parser.
3. **Zero-Dependency, Single-Pass Scanning (`P-STDLIB-ONLY`, `P-NO-QUADRATIC`):** PSL is designed to be parsed in a single $O(N)$ pass using pure Rust standard library slices (`&str`, `Cow<'a, str>`) without regular expressions or external crates.
4. **String-Centric Tokens:** At the syntax level, all identifiers and arguments are UTF-8 string tokens. Semantic type conversion (such as booleans, port numbers, paths, or `@group` references) is delegated to the domain validation layer.

## 2. Lexical Elements

### 2.1 Character Encoding
- A PSL document MUST be a valid UTF-8 byte sequence.
- Invalid UTF-8 byte sequences MUST be rejected before or during scanning.
- The scanner MUST respect UTF-8 character boundaries at all times (`P-UTF8-LOOKAROUND`).

### 2.2 Whitespace
Horizontal whitespace separates tokens within a single line and is otherwise insignificant outside of quoted strings.
- **Allowed horizontal whitespace characters:**
  - ASCII Space (`U+0020`, `' '`)
  - ASCII Horizontal Tab (`U+0009`, `'\t'`)
- Other Unicode whitespace characters outside of quoted strings MUST be rejected as invalid characters.

### 2.3 Newlines (Line Terminators)
PSL is a line-oriented language. Directives and block headers are terminated by a newline.
- **Allowed newline sequences:**
  - Line Feed (`LF`, `U+000A`, `"\n"`)
  - Carriage Return + Line Feed (`CRLF`, `U+000D U+000A`, `"\r\n"`)
- A standalone Carriage Return (`CR`, `U+000D`) not followed by `LF` outside of a double-quoted escape sequence is invalid.
- Empty lines (lines containing only optional horizontal whitespace and an optional comment) are ignored anywhere in the document.

### 2.4 Comments
- A comment starts with the hash character (`#`, `U+0023`) outside of a quoted string.
- All characters from `#` up to (but not including) the next newline (`LF`, `CRLF`) or End-Of-File (`EOF`) are ignored by the parser.
- A `#` character inside a bare word (e.g., `foo#bar`) is NOT part of the bare word; it immediately terminates the token and starts an inline comment (`foo` followed by comment `#bar`).
- Multi-line block comments are not supported.

### 2.5 Delimiters & Reserved Punctuation
The following ASCII characters have special syntactic meaning outside of quoted strings:
- `{` (`U+007B`): Opens a block body.
- `}` (`U+007D`): Closes a block body.
- `#` (`U+0023`): Starts a line or inline comment.
- `"` (`U+0022`): Delimits a double-quoted string.
- `'` (`U+0027`): Delimits a single-quoted literal string.

### 2.6 Identifiers (Block & Directive Names)
An **Identifier** is the first token of a block header or directive.
- An Identifier MUST be an unquoted token (bare word). Quoted strings are not permitted as block or directive names.
- **Allowed characters:** ASCII alphanumeric characters (`a-z`, `A-Z`, `0-9`), underscore (`_`), and hyphen (`-`).
- **First character rule:** An Identifier MUST start with an ASCII alphabetic character (`a-z`, `A-Z`) or underscore (`_`). It MUST NOT start with a digit (`0-9`) or hyphen (`-`).

### 2.7 Arguments (Values)
An **Argument** represents a parameter passed to a block header or a directive. An argument can be written in one of three forms:

#### A. Bare Word (Unquoted String)
- A contiguous sequence of one or more valid UTF-8 characters excluding horizontal whitespace (`' '`, `'\t'`), newlines (`\r`, `\n`), and reserved punctuation (`{`, `}`, `#`, `"`, `'`).
- Bare words allow domain-specific symbols without quoting, including `/`, `.`, `:`, `-`, `_`, `*`, `@`, and `=` (e.g., `/var/lib/db/data:Z`, `*db`, `@database`, `--userns=keep-id`).
- A bare word is borrowed directly from the input buffer as a zero-copy `&str` slice.

#### B. Double-Quoted String (`"..."`)
- Begins and ends with `"` (`U+0022`).
- May contain horizontal whitespace, `#`, `{`, `}`, and `'`.
- **Single-line constraint:** Unescaped raw newlines (`\r` or `\n`) inside a double-quoted string are forbidden and MUST trigger an `UnterminatedString` error.
- **Escape sequences:** Backslash (`\`, `U+005C`) escapes the immediately following character:
  - `\"` $\rightarrow$ Quotation mark (`U+0022`)
  - `\\` $\rightarrow$ Reverse solidus / backslash (`U+005C`)
  - `\n` $\rightarrow$ Line feed (`U+000A`)
  - `\r` $\rightarrow$ Carriage return (`U+000D`)
  - `\t` $\rightarrow$ Horizontal tab (`U+0009`)
- Any other backslash escape sequence MUST be rejected with an `InvalidEscapeSequence` error.
- If a double-quoted string contains no escape sequences, the parser SHOULD return a zero-copy `Cow::Borrowed(&str)` slice excluding the outer quotes.

#### C. Single-Quoted String (`'...'`)
- Begins and ends with `'` (`U+0027`).
- Treated as a literal (raw) single-line string: backslashes (`\`) and double quotes (`"`) are treated as literal characters without escape processing.
- **Single-line constraint:** Raw newlines (`\r` or `\n`) inside a single-quoted string are forbidden.
- Always returns a zero-copy `Cow::Borrowed(&str)` slice excluding the outer quotes.

## 3. Structural Grammar

### 3.1 Document Structure (Mandatory Root Blocks)
A PSL document represents a sequence of **Blocks**.
- At the root level (nesting depth `0`), ONLY **Blocks**, blank lines, and comments are permitted.
- **Root-Level Directive Prohibition:** Any directive appearing at nesting depth `0` (outside of a `{ ... }` block) MUST be rejected as a syntax error (`UnexpectedRootDirective`).
- An empty file (or a file containing only whitespace and comments) is a valid empty PSL document with zero blocks.

### 3.2 Blocks
A **Block** groups related directives and nested sub-blocks under a named context.

```
<identifier> [<argument> ...] {
    <statement> ...
}
```

- **Block Header:** Consists of an Identifier, followed by zero or more whitespace-separated Arguments, followed by the opening brace `{`.
- **Opening Brace Placement:** The opening brace `{` MUST appear on the same line as the block Identifier and its arguments, separated from the preceding token by at least one horizontal whitespace character (e.g., `general {` or `pod example-stack {`).
- **After Opening Brace:** Only optional horizontal whitespace and an optional `#` comment may follow `{` before the line terminator (`\n` or `\r\n`), except for an empty inline block `{}`.
- **Block Body:** Contains zero or more statements (Directives or nested Blocks), each on its own logical line.
- **Closing Brace Placement:** A non-empty block is closed by `}` on its own logical line (preceded only by optional horizontal whitespace and followed only by optional horizontal whitespace, an optional `#` comment, and a newline or `EOF`).
- **Empty Blocks:** A block may be empty, written either across multiple lines or inline as `<identifier> [<argument> ...] {}`.

### 3.3 Directives
A **Directive** is a single-line key-value or key-tuple instruction inside a block.

```
<identifier> <argument> [<argument> ...]
```

- **Block-Only Scope:** A directive MUST reside inside a block (nesting depth $\ge 1$).
- **Minimum Arity:** A directive MUST have an Identifier followed by at least one Argument separated by horizontal whitespace. A standalone identifier on a line without arguments and without `{` is a syntax error (`MissingDirectiveArgument`).
- **Termination:** A directive is terminated by a newline (`LF` or `CRLF`) or `EOF` (if preceding a closing brace). Semicolons (`;`) are NOT used as statement terminators; a trailing `;` on an unquoted token is treated as part of that bare word.
- **Ordering & Multiplicity:** The parser MUST preserve the exact top-to-bottom declaration order of blocks and directives. Repeated directives with the same identifier (e.g., multiple `publish`, `volume`, or `option` lines) are syntactically valid and appended in sequence.

## 4. Formal Grammar (EBNF)

```ebnf
(* Document Root: Strictly Blocks, Comments, and Blank Lines *)
document        = { empty-line | root-block } ;

root-block      = block ;

(* Blocks *)
block           = ws* identifier { ws+ argument } ws+ "{" ws* [ comment ] newline
                  { block-item }
                  ws* "}" ( [ ws* [ comment ] newline ] | ws* [ comment ] eof )
                | ws* identifier { ws+ argument } ws+ "{" ws* "}"
                  ( [ ws* [ comment ] newline ] | ws* [ comment ] eof ) ;

block-item      = empty-line
                | block
                | directive ;

(* Directives: Must contain at least one argument *)
directive       = ws* identifier { ws+ argument }+ ws* [ comment ] ( newline | eof ) ;

(* Tokens *)
identifier      = id-start { id-continue } ;
id-start        = "a".."z" | "A".."Z" | "_" ;
id-continue     = id-start | "0".."9" | "-" ;

argument        = quoted-string
                | single-string
                | bare-word ;

bare-word       = bare-char { bare-char } ;
bare-char       = ? any UTF-8 char except ' ', '\t', '\r', '\n', '{', '}', '#', '"', '\'' ? ;

quoted-string   = '"' { quoted-char | escape-seq } '"' ;
quoted-char     = ? any UTF-8 char except '"', '\\', '\r', '\n' ? ;
escape-seq      = "\\" ( '"' | "\\" | "n" | "r" | "t" ) ;

single-string   = "'" { single-char } "'" ;
single-char     = ? any UTF-8 char except '\'', '\r', '\n' ? ;

(* Whitespace & Comments *)
empty-line      = ws* [ comment ] newline ;
comment         = "#" { ? any UTF-8 char except '\r', '\n' ? } ;
ws              = " " | "\t" ;
newline         = "\n" | "\r\n" ;
```

## 5. Security & Resource Bounds

To comply with `AGENTS.md` (`P-BOUND-ALLOC`, `P-NO-QUADRATIC`, `P-UTF8-LOOKAROUND`), the PSL parser in `src/io/psl/` MUST enforce the following deterministic limits using named constants:

| Constant Name          | Default Limit       | Purpose |
| :--------------------- | :------------------ | :------ |
| `MAX_FILE_BYTES`       | `1_048_576` (1 MiB) | Maximum input size in bytes before rejecting input to prevent OOM. |
| `MAX_NESTING_DEPTH`    | `8`                 | Maximum `{ ... }` block nesting depth (prevents stack overflow). |
| `MAX_BLOCKS_PER_DOC`   | `256`               | Maximum total blocks per PSL document. |
| `MAX_STMTS_PER_BLOCK`  | `512`               | Maximum directives/sub-blocks inside a single block. |
| `MAX_ARGS_PER_STMT`    | `64`                | Maximum arguments allowed on a single block header or directive. |

## 6. Mandatory Error Conditions

The PSL parser MUST return a strongly typed error enum (including the 1-based line number) and MUST fail deterministically on any of the following inputs:

- **Root Directive (`UnexpectedRootDirective`):** A key-value directive (e.g., `editor vim`) appears at nesting depth `0` outside of any `{ ... }` block.
- **Missing Argument or Brace (`MissingDirectiveArgument`):** A single identifier appears on a line without arguments and without an opening `{`.
- **Quoted Identifier (`InvalidIdentifier`):** A block or directive name is quoted (e.g., `"general" {` or `"editor" vim`) or starts with an invalid character (e.g., `123block {` or `-flag val`).
- **Missing Whitespace Before Brace (`InvalidBlockHeader`):** The opening `{` is attached directly to the identifier or argument without whitespace (e.g., `general{` or `pod app{`).
- **Trailing Tokens After Brace (`UnexpectedToken`):** Non-comment tokens appear on the same line after `{` (except `}`) or after `}`.
- **Unclosed Block (`UnclosedBlock`):** `EOF` is reached while nesting depth $> 0$.
- **Unmatched Closing Brace (`UnexpectedClosingBrace`):** `}` is encountered at nesting depth `0`.
- **Unterminated Quote (`UnterminatedString`):** A `"` or `'` string reaches `\r`, `\n`, or `EOF` before its matching closing quote.
- **Adjacent Quoted Token (`UnexpectedToken`):** A quote opens or closes in the middle of a token without separating whitespace (e.g., `foo"bar"` or `"foo"bar`).
- **Invalid Escape (`InvalidEscapeSequence`):** A `"` string contains an unknown escape sequence (e.g., `\a`, `\x`, `\0`).
- **Limit Exceeded (`LimitExceeded`):** Any resource bound defined in Section 5 is exceeded.

## 7. Canonical Examples

### 7.1 Application Configuration (`pods.conf`)

```
# Podoxide Configuration (PSL)

general {
    editor   vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all      missing_recipe
        stack    @database @web
        database recipe_001_empty
        web      frontend
    }
}
```

### 7.2 Pod Recipe (`example.pod`)

```
# Example Web Stack Recipe

pod example-stack {
    name        "Example Web Stack"
    description "A minimal but complete example recipe with a web app and database."
    path        /pods/example-stack
    registry    docker.io
    publish     8080 80 TCP
    option      --userns=keep-id

    command migrate {
        description "Run database migrations."
        container   *db
        user        dbuser
        execute     "db-cli -U dbuser -d app -f /migrations/run.sql"
    }
}

container *db {
    detach   true
    restart  always
    registry docker.io
    image    example/db-server:latest
    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'
    option   "--env DB_PASSWORD=secret"
}

container *app {
    detach  true
    restart always
    image   example/example-app:latest
    command start-server.sh
}
```

## 8. Agent Workflow: Spec-Driven Modification Protocol

Whenever the user modifies `PSL_SPEC.md` or instructs you to change the PSL syntax, execute these exact steps in order:

1. **Diff & Analyze Spec:** Read `PSL_SPEC.md` and identify the exact lexical, grammatical, or bound changes in Sections 2 through 6.
2. **Update Tests First (TDD Red Phase):**
   - Locate the PSL test suite (unit tests in `src/io/psl/` and integration tests in `tests/`).
   - Add or update positive test cases for new valid syntax and negative test cases for new error conditions (`M-DELIM-INJECT-TESTS`, `M-EXTREME-BOUND-TESTS`, `M-FALLBACK-TESTS`).
   - Run `cargo test` to verify that the new/updated tests fail as expected against the old parser.
3. **Update Parser Implementation (TDD Green Phase):**
   - Modify the scanner and parser in `src/io/psl/` to satisfy the updated `PSL_SPEC.md` while strictly adhering to `AGENTS.md` layout and memory rules (`P-UNIFIED-PARSER`, `P-NO-QUADRATIC`, `P-BOUND-ALLOC`, `P-UTF8-LOOKAROUND`).
4. **Update Templates & Docs:** If the syntax change affects `pods.conf` or `*.pod` templates, update root templates and documentation per `update-documentation`.
5. **Verify:** Run `cargo test` and `cargo clippy -- -D warnings` until all checks pass with zero warnings.
