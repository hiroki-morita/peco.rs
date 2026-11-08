# peco.rs

RustでPEG（Parsing Expression Grammar）のパーサを組み立てるライブラリです。基本的なパーサを関数やメソッドで合成し、変数に束縛して再利用できます。再帰・相互再帰ルールと、ユーザ定義のstructやenumを返すパース結果を扱えます。

プロジェクト名は **peco.rs**、Cargoパッケージ名とRustのクレート名は **`peco`** です。実行時の外部依存はありません。

## 導入

利用するプロジェクトの `Cargo.toml` に、ローカルチェックアウトへのパスを指定します。`path` は利用側の `Cargo.toml` からの相対パスに置き換えてください。

```toml
[dependencies]
peco = { path = "../peco.rs" }
```

Rustコードでは `use peco::...` としてインポートします。このライブラリはRust edition 2024を使用しています。

## 最初の例

`count=42` を読み取り、独自の構造体として返す例です。

```rust
use peco::{ParseError, literal, satisfy};

#[derive(Debug, PartialEq)]
struct Assignment {
    name: String,
    value: u64,
}

fn main() -> Result<(), ParseError> {
    let name = satisfy("ASCII letter", |c| c.is_ascii_alphabetic())
        .plus()
        .text();
    let value = satisfy("ASCII digit", |c| c.is_ascii_digit())
        .plus()
        .text()
        .try_map(|s| s.parse::<u64>());
    let assignment = name
        .then_ignore(literal("="))
        .then(value)
        .map(|(name, value)| Assignment { name, value });

    assert_eq!(
        assignment.parse("count=42")?,
        Assignment { name: "count".into(), value: 42 },
    );
    Ok(())
}
```

`plus()` は1回以上の繰り返し、`text()` は一致した入力の文字列化、`try_map()` は失敗する可能性のある変換、`map()` は成功結果の変換を表します。空白は自動で読み飛ばさないため、この例の `count = 42` は不一致になります。

## 基本式

以下では `p: Parser<A>`、`q: Parser<B>`、`r: Parser<A>` とします。

| PEG | API | 出力型 |
| --- | --- | --- |
| 文字列 `"abc"` | `literal("abc")` | `()` |
| 文字クラス `[a-z]` | `satisfy("lowercase", predicate)` | `char` |
| 任意文字 `.` | `any_char()` | `char` |
| 連接 `p q` | `p.then(q)` | `(A, B)` |
| 順序付き選択 `p / r` | `p.or(r)` | `A` |
| 0回以上 `p*` | `p.star()` | `Vec<A>` |
| 1回以上 `p+` | `p.plus()` | `Vec<A>` |
| オプション `p?` | `p.optional()` | `Option<A>` |
| 肯定先読み `&p` | `and(p)` | `()` |
| 否定先読み `!p` | `not(p)` | `()` |
| 非終端記号 | `recursive("Rule", builder)` | ルールの出力型 |

各APIは表の出力型を持つ `Parser<...>` を返します。先読みの `and` / `not` は自由関数で、入力を消費しません。

文字の指定には `char('x')`、`char_range('a'..='z')`、`one_of("abc")` も利用できます。`end()` は入力末尾で成功します。文字はUnicodeスカラー値単位で扱い、`any_char()` は改行にも一致します。

## 合成と再利用

コンビネータはパーサを所有するので、複数箇所で使う場合は `.clone()` します。cloneは実行器などの共有参照を複製し、出力型に `Clone` を要求しません。実行には `&self` を使うので、構築したパーサは何度でも実行できます。

```rust
use peco::{ParseError, char, satisfy};

fn main() -> Result<(), ParseError> {
    let digit = satisfy("ASCII digit", |c| c.is_ascii_digit());
    let signed = char('-').optional().then(digit.clone().plus());

    assert_eq!(signed.parse("-42")?, (Some('-'), vec!['4', '2']));
    assert_eq!(signed.parse("7")?, (None, vec!['7']));
    assert_eq!(digit.parse("9")?, '9');
    Ok(())
}
```

`or` の両分岐は同じ出力型が必要です。異なる種類の値を選択する場合は、`map` で共通のenumに変換してから結合します。

## パース結果の変換

| API | 用途 |
| --- | --- |
| `p.map(f)` | 成功値を別の型へ変換する |
| `p.try_map(f)` | `Result` を返す関数で変換し、変換失敗を通常の不一致にする |
| `p.text()` | 元の出力を捨て、一致範囲を所有する `String` として取得する |
| `p.spanned()` | 元の出力と `Span { start, end }` を取得する |
| `p.ignore()` | 出力を捨てて `()` にする |
| `p.then_ignore(q)` | 両方を実行して `p` の出力を残す |
| `p.ignore_then(q)` | 両方を実行して `q` の出力を残す |

`Span` は開始位置を含み、終了位置を含まないUTF-8バイト範囲です。変換は純粋な関数として記述してください。先読みや最終的に失敗する分岐でも変換が実行されることがあり、外部への副作用は巻き戻されません。

## 再帰ルール

`recursive` は、構築中のルールへの参照をクロージャに渡します。括弧の入れ子をenumとして返す例です。

```rust
use peco::{ParseError, literal, recursive, satisfy};

#[derive(Debug, PartialEq)]
enum Expr {
    Number(u64),
    Group(Box<Expr>),
}

fn main() -> Result<(), ParseError> {
    let number = satisfy("ASCII digit", |c| c.is_ascii_digit())
        .plus()
        .text()
        .try_map(|s| s.parse::<u64>())
        .map(Expr::Number);
    let expr = recursive("Expr", |expr| {
        literal("(")
            .ignore_then(expr)
            .then_ignore(literal(")"))
            .map(|value| Expr::Group(Box::new(value)))
            .or(number)
    });

    assert_eq!(expr.parse("(42)")?, Expr::Group(Box::new(Expr::Number(42))));
    Ok(())
}
```

### 相互再帰

相互再帰には `GrammarBuilder` を使います。たとえば次の2ルールは互いを参照し、`A` を開始ルールにすると `a`、`ab`、`aba` のような交互の文字列に一致します。

```text
A <- "a" B?
B <- "b" A?
```

先に両方を `declare` して `reference` を取得すると、定義順に依存せず本体を組み立てられます。

```rust
use peco::{GrammarBuilder, literal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut grammar = GrammarBuilder::new();
    let a = grammar.declare::<()>("A");
    let b = grammar.declare::<()>("B");
    let a_ref = grammar.reference(&a)?;
    let b_ref = grammar.reference(&b)?;

    grammar.define(&a, literal("a").then(b_ref.optional()).ignore())?;
    grammar.define(&b, literal("b").then(a_ref.clone().optional()).ignore())?;
    let parser = grammar.finish(a_ref)?;

    for input in ["a", "ab", "aba", "ababa"] {
        assert_eq!(parser.parse(input)?, ());
    }
    for input in ["", "aa", "ba"] {
        assert!(parser.parse(input).is_err());
    }
    Ok(())
}
```

この例では一致判定だけを行うため、出力型を `()` とし、`ignore()` で連接の結果を捨てています。次のルールを呼ぶ前に必ず1文字消費するので、左再帰にはなりません。`finish` は全ルールの定義などを検証し、文法を保持する実行可能なルートを返します。

[設計文書](docs/ARCHITECTURE.md)と[統合テスト](tests/public_api.rs)にも詳細があります。

## 実行とエラー

`parse` は入力全体の消費を要求します。部分一致には `parse_prefix` を使い、結果の `value` と、消費したバイト数 `consumed` を取得します。

```rust
use peco::{ParseError, literal};

fn main() -> Result<(), ParseError> {
    let parser = literal("猫");
    let prefix = parser.parse_prefix("猫!")?;
    assert_eq!(prefix.consumed, 3);

    let input = "猫!";
    let error = parser.parse(input).unwrap_err();
    assert_eq!(
        error.display_with_input(input).to_string(),
        "expected end of input at byte 3 (line 1, column 2)",
    );
    Ok(())
}
```

`ParseError::Mismatch` は通常の入力不一致、`ParseError::Fatal` は文法不正や実行制限のエラーです。選択・反復・否定先読みも致命的エラーを伝播します。`label("名前")` で期待項目の名前を指定できます。行・列の表示には元の入力を渡してください。行・列は1始まりで、列はUnicodeスカラー値単位です。

## PEGの動作と制約

- **順序付き選択**: 左が通常失敗した場合だけ、元の位置から右を試します。左が成功した後に後続が失敗しても、右を選び直しません。
- **貪欲な反復**: 成功する限り繰り返し、消費済みの入力を後続へ返しません。消費なしの成功を繰り返すと `NonProgressingRepetition` になります。
- **空白と文字**: 空白・コメントは明示的に合成します。Unicode正規化や大文字小文字の変換は自動では行いません。
- **再帰**: 直接・間接左再帰は `LeftRecursion` になります。`Expr <- Expr "+" Term / Term` は `Term ("+" Term)*` として表現し、結果を左foldして左結合の木を構築します。
- **深さ上限**: 同時ルール呼出しの既定上限は64です。`ParseOptions { recursion_limit }` を `parse_with_options` / `parse_prefix_with_options` に渡して変更できます。上限を引き上げた場合のスタック容量は保証しません。
- **出力とスレッド**: 出力は `T: 'static` を満たす所有値や `Span` を使います。入力自体に `'static` は不要ですが、入力を借用する出力は扱いません。`Rc` を利用するため `Parser<T>` は `Send` / `Sync` を実装しません。
- **計算量**: packratメモ化は行っておらず、線形時間は保証しません。

## 利用例と開発

[算術式の例](examples/arithmetic.rs)は括弧、空白、整数の加減乗算を扱い、優先順位と左結合を反映したASTを構築します。

```sh
cargo run --example arithmetic -- '10 - 2 * (3 + 1)'
# 出力: 2
```

開発時の検証とAPIドキュメントの生成は、リポジトリのルートで実行します。

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps --open
```

- [アーキテクチャと実行仕様](docs/ARCHITECTURE.md)
- [実装タスクと検証記録](docs/tasks.md)
- [公開APIの統合テスト](tests/public_api.rs)
