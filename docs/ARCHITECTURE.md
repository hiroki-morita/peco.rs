# PEGパーサコンビネータの設計

## 1. 目的と設計方針

RustでParsing Expression Grammar（PEG）の式を合成し、ユーザ定義型の値を返すパーサを構築する。本書は初版の設計仕様を表す。実装の進捗は `tasks.md` を参照する。

初版では、型消去した共有可能な `Parser<T>`、所有する出力値、単一スレッドでの実行を採用する。利用者が組合せ後の巨大な型を扱う必要がなく、変数への束縛・再利用・再帰の表現を優先できる。PEGは順序付き選択を持ち、最初に成功した分岐を採用する。曖昧性の列挙や最長一致選択は行わない。

## 2. 基本式と出力型

以下で `p: Parser<A>`、`q: Parser<B>`、`r: Parser<A>` とする。

| PEGの式 | 提案API | 出力型 | 振る舞い |
| --- | --- | --- | --- |
| 文字列 `"abc"` | `literal("abc")` | `Parser<()>` | 現在位置から文字列全体に一致。空文字列も成功 |
| 文字クラス `[a-z]` | `satisfy("ASCII lowercase", predicate)` | `Parser<char>` | Unicodeスカラー値1個を述語で判定 |
| 任意文字 `.` | `any_char()` | `Parser<char>` | 改行も含む1文字。EOFでは失敗 |
| 連接 `p q` | `p.then(q)` | `Parser<(A, B)>` | 左から順に実行 |
| 順序付き選択 `p / r` | `p.or(r)` | `Parser<A>` | 左が通常失敗した場合だけ同じ位置から右を実行 |
| 0回以上 `p*` | `p.star()` | `Parser<Vec<A>>` | 貪欲に反復し、通常失敗で停止 |
| 1回以上 `p+` | `p.plus()` | `Parser<Vec<A>>` | 1回目の成功を必須とする反復 |
| オプション `p?` | `p.optional()` | `Parser<Option<A>>` | 通常失敗を消費なしの `None` に変換 |
| 肯定先読み `&p` | `and(p)` | `Parser<()>` | `p` が成功すれば消費せず成功 |
| 否定先読み `!p` | `not(p)` | `Parser<()>` | `p` が通常失敗すれば消費せず成功 |
| 非終端記号 `Rule` | `recursive("Rule", builder)` など | `Parser<A>` | 名前付きルールを参照 |

文字クラスの便利関数として `char_range('a'..='z')`、`one_of("abc")`、`char('x')` を提供する。逆順の文字範囲と空の文字集合は何にも一致しない。文字列の途中不一致は一致済み接頭辞の直後を診断位置とする（呼出し側の入力位置は消費しない）。文字クラスはバイトや書記素クラスタ単位ではない。Unicode正規化や大文字小文字変換は暗黙に行わない。

値を必要としない文字列の出力は `()` とし、消費したテキストは `p.text(): Parser<String>`、位置情報は `p.spanned(): Parser<(A, Span)>` で取得する。`text()` は内部の出力を捨て、成功時の開始・終了位置から文字列を複製する。

## 3. 公開APIと合成

```rust
// 概念上の公開型。内部フィールドは公開しない。
pub struct Parser<T> { /* Rcで実行器を共有 */ }
pub struct Span { pub start: usize, pub end: usize }
pub struct ParseOutput<T> { pub value: T, pub consumed: usize }

impl<T: 'static> Parser<T> {
    pub fn parse(&self, input: &str) -> Result<T, ParseError>;
    pub fn parse_prefix(&self, input: &str)
        -> Result<ParseOutput<T>, ParseError>;
    pub fn map<U: 'static>(self, f: impl Fn(T) -> U + 'static) -> Parser<U>;
    // ほかに then / or / star / plus / optional など
}

// 先読みはメソッドではなく自由関数として提供する。
pub fn and<T: 'static>(parser: Parser<T>) -> Parser<()>;
pub fn not<T: 'static>(parser: Parser<T>) -> Parser<()>;
```

`parse` は入力全体の消費を必須とし、末尾が残ればEOFを期待したエラーを返す。`parse_prefix` は部分一致を許し、消費したUTF-8バイト数を返す。空白・コメントは自動では読み飛ばさない。必要なら空白パーサを明示的に合成する。

`Parser<T>` の `Clone` は `Rc` の複製だけであり、`T: Clone` を要求しない。コンビネータは `self` を消費して新たなパーサを構築するので、同じパーサを複数箇所で使う場合は `.clone()` する。`and(p)` / `not(p)` も引数のパーサを所有するため、再利用時は `and(p.clone())` のように渡す。実行には `&self` を使い、同じパーサを何度でも実行できる。

```rust
let digit = satisfy("ASCII digit", |c| c.is_ascii_digit());
let digits = digit.clone().plus();
let signed_digits = char('-').optional().then(digits);
```

異なる型を返す選択肢は、`map` で共通のenumなどに変換してから `or` で結合する。補助APIとして `p.ignore(): Parser<()>`、`p.then_ignore(q): Parser<A>`、`p.ignore_then(q): Parser<B>`、`end(): Parser<()>` を用意する。

初版では出力を `T: 'static` に制限し、入力を借用する `&str` を出力しない。これは入力を静的寿命に制限する意味ではない。入力自体は通常の `&str` でよい。コピーを避けたい利用者は `Span` を返して、呼出し側が保持する入力を参照する。入力寿命に依存する出力は別の高度なAPIとして将来検討する。

## 4. 実行モデルとバックトラック

内部の実行器は概念的に次の形とする。

```rust
type Exec<T> = dyn for<'i> Fn(&mut Context<'i>, usize) -> Step<T>;
// Parser<T> は Rc<Exec<T>> を保持する。
// Context<'i> は &'i str、実行中のルール集合、深さ上限などを保持する。
type Step<T> = Result<(T, usize), Failure>;
enum Failure {
    Mismatch(Diagnostic), // 入力との通常の不一致
    Fatal(GrammarError),  // 文法不正、実行制限など
}
```

入力位置を引数・戻り値で明示する。成功した場合だけ次の位置が伝わり、失敗によって呼出し側の位置は変わらない。すべての位置はUTF-8バイトオフセットで、常に文字境界を指す。文字パーサは `char::len_utf8()` 分だけ進む。`Context` はパース呼出しごとに生成し、パーサ本体に実行位置を保持しない。

連接は左の成功位置から右を実行する。右が失敗した場合、連接全体が失敗する。選択肢が途中まで消費してから失敗しても、`or` は元の開始位置から右を試す。明示的な `attempt` は不要である。

一方、成功済みの選択や繰り返しは後続の都合でやり直さない。たとえば `(literal("a").or(literal("ab"))).then(literal("c"))` は `abc` に失敗する。最初の選択が `a` で成功した後、`c` が失敗しても `ab` を選び直さない。また `literal("a").star().then(literal("a"))` は `aaa` に失敗する。反復が消費した末尾の `a` を後続へ返さない。

通常失敗だけを、選択・オプション・繰り返し・否定先読みの制御に利用する。`Fatal` はすべてのコンビネータを通して伝播し、否定先読みでも成功に反転しない。先読み内で作成した値は破棄し、入力位置は必ず開始位置に戻す。

## 5. 再帰と非終端記号

### 単一ルールの再帰

```rust
pub fn recursive<T: 'static>(
    name: impl Into<String>,
    build: impl FnOnce(Parser<T>) -> Parser<T>,
) -> Parser<T>;
```

`build` に渡す引数は未完成ルールへの参照であり、返した本体がそのルールを定義する。Rustの変数初期化中に自分自身を参照する必要がない。定義は構築時に一度だけ行い、入力ごとに再構築しない。

```rust
#[derive(Debug, PartialEq)]
enum Expr {
    Number(u64),
    Group(Box<Expr>),
}

let number = satisfy("ASCII digit", |c| c.is_ascii_digit())
    .plus()
    .text()
    .try_map(|s| s.parse::<u64>())
    .map(Expr::Number);

let expr = recursive("Expr", |expr| {
    let group = literal("(")
        .ignore_then(expr)
        .then_ignore(literal(")"))
        .map(|value| Expr::Group(Box::new(value)));
    number.or(group)
});
let value = expr.parse("((42))")?;
```

これは右辺に通常の式を渡すAPI例であり、文法マクロやコード生成を必要としない。

### 相互再帰と所有権

相互再帰には `GrammarBuilder` を設ける。`declare::<T>(name)` で型付き `Rule<T>` を作り、`reference(&rule)` で `Parser<T>` を得て、`define(&rule, body)` で本体を一度登録する。`finish(root)` が未定義ルールや重複定義、別のビルダーのルール参照を検証し、完成した `Parser<T>` を返す。構築途中の参照を実行すると通常不一致ではなく構築エラーを返す。

内部では、文法所有者が異なる出力型のルールセルを型消去して保持し、型付きハンドルが対応するセルを指す。型の対応はビルダーが保証し、利用者にダウンキャストを要求しない。セルには一度だけ本体を設定する `OnceCell` 相当を用いる。

完成したルートは文法所有者への強参照を保持し、本体内の非終端参照はセルへの弱参照を保持する。これにより `文法 → 本体 → 非終端 → 文法` の強参照循環を避ける。ルートのcloneやルートを含む合成パーサも所有者を保持する。所有者が消滅した内部参照を単独で実行した場合は明示的なエラーとし、panicにしない。`recursive` はこの仕組みの単一ルール用の簡易APIとする。

### 左再帰と停止性

初版は直接・間接左再帰をサポートしない。実行中の `(RuleId, input_offset)` を記録し、同じ組が完了前に再登場したら `LeftRecursion` を返す。成功・失敗どちらでも記録を除去する。先読み経由の同一位置再帰も検出する。

たとえば `Expr <- Expr "+" Term / Term` は `Expr <- Term ("+" Term)*` に書き換え、得られた列を左foldして左結合ASTを構築する。右再帰や括弧の再帰は、再帰呼出しまでに入力が進めば利用できる。

`star` / `plus` は各成功時に入力位置の増加を検証する。`literal("").star()` や `p.optional().star()` が消費なしで成功した場合、無限ループや黙った打切りではなく `NonProgressingRepetition` を返す。入力を進める再帰にも深さ上限を設け、スタック枯渇前に `RecursionLimit` を返す。初期値は保守的に設定して対象環境で検証し、実行オプションで変更可能にする。

## 6. パース結果からユーザ定義型への変換

`map` は構文の出力値を任意の型へ変換する。連接のタプル、反復の `Vec`、オプションの `Option` を材料に、structやenumを構築できる。

```rust
#[derive(Debug)]
struct Assignment {
    name: String,
    value: u64,
}

let name = satisfy("ASCII letter", |c| c.is_ascii_alphabetic())
    .plus().text();
let value = satisfy("ASCII digit", |c| c.is_ascii_digit())
    .plus().text().try_map(|s| s.parse::<u64>());
let assignment = name
    .then_ignore(literal("="))
    .then(value)
    .map(|(name, value)| Assignment { name, value });
let result = assignment.parse("count=42")?;
```

`try_map` は `Fn(T) -> Result<U, E>`（`E: Display`）を受け、数値の範囲外などの変換失敗を、そのパーサの消費範囲と終了位置を含む通常不一致へ変換する。したがって外側の選択は別の分岐を試せる。この振る舞いをAPI仕様として固定する。変換エラー文字列は内部で所有し、元のエラー型を公開出力型へ伝播させない。

結果を外部の可変コレクションへ書き込む専用アクションは設けない。失敗した分岐で作った値は破棄し、成功した値だけを親へ返す。`map` / `try_map` は `Fn` とし、純粋な変換として使うことを要求する。Rustの内部可変性による副作用までは禁止できず、それらをロールバックする保証もない。先読みや失敗する分岐でも変換が実行され得るため、外部への書込みは最終的な `parse` 成功後に行う。

## 7. エラー設計

公開 `ParseError` は入力不一致と文法・実行エラーを区別する。入力不一致には最遠失敗位置、期待項目の集合、ルール名の文脈、必要に応じた変換失敗メッセージと範囲を格納する。行・列は表示時に入力から計算し、列はUnicodeスカラー値単位とする。タブの表示幅は計算しない。

診断は各失敗の戻り値に持たせる。グローバルな「最遠位置」を無条件に更新しない。

- `or` の両方が通常失敗した場合は、遠い位置の診断を採用する。同位置なら期待項目を重複除去して併合する。
- `or` が成功した場合は、試行済みの失敗分岐の診断を破棄する。
- オプションの欠如、反復の正常終了、否定先読みの成功に使った失敗は診断から除外する。
- `plus` の初回失敗は呼出し側に返す。
- 肯定先読みの失敗は内部診断を返す。否定先読みの失敗は開始位置に「禁止された式に一致」と記録する。
- `label(name)` で期待項目を利用者向けの名前に置換できる。ルール呼出しは診断にルール文脈を付加する。

この方針は診断の予測可能性を優先する。成功した分岐に隠れた失敗候補まで保存する、高度な診断探索は初版では行わない。

## 8. モジュールと実装順序

| モジュール | 責務 |
| --- | --- |
| `parser` | `Parser<T>`、実行コンテキスト、全体・部分パース |
| `primitive` | 文字列、文字クラス、任意文字、EOF |
| `combinator` | 連接、選択、反復、オプション、先読み |
| `transform` | `map`、`try_map`、`text`、`spanned`、出力破棄 |
| `rule` | ルール宣言・定義、再帰、所有権、停止性検査 |
| `error` | 診断の併合、公開エラー、表示 |

まず位置と失敗のモデル、次に基本式と変換、最後に再帰ビルダーを実装する。`lib.rs` から公開APIを再エクスポートする。標準ライブラリだけで初版を構成できる。

型消去には動的ディスパッチと構築時の割当が伴う。実行時にも出力用 `Vec` / `String` の割当が発生する。初版はpackratメモ化を行わず、PEGであっても線形時間は保証しない。重なる選択と再帰によって試行回数が大きく増えることがある。メモ化には異種出力値の保存、複製または共有、変換実行回数の仕様が必要なので、実測後に別途設計する。`Rc` ベースのため `Send` / `Sync` は保証しない。静的型の実行器や `Arc` 版、左再帰対応、エラー回復も初版の対象外とする。

## 9. 検証計画

実装時には、正常系だけでなくPEG固有の制御と所有権を検証する。

- 全基本式の成功・失敗・EOF、空入力、改行、マルチバイト文字とバイト位置。
- 途中で失敗した左選択肢からの巻戻し、成功済み選択の固定、反復の貪欲性。
- 先読みの非消費、オプションの巻戻し、0回と1回以上の反復の違い。
- 消費しない反復と直接・間接左再帰が明示エラーになり、選択などで隠れないこと。
- 変数への束縛、clone後の再利用、`Clone` を実装しない出力型の利用。
- `map` によるstruct・enumの生成、`try_map` の失敗後の選択、Spanの正確性。
- 単一再帰・相互再帰、未定義・重複定義・別文法の参照、深さ制限。
- ルートのclone保持中は再帰が動作し、全ルート破棄後にセルと本体が解放されること。
- 診断の最遠位置・同位置併合と、反復停止や成功した選択からの不要な診断の除外。
- `parse` の末尾検査と `parse_prefix` の消費数、および同じパーサの複数回実行。

掲載例はAPI実装後にドキュメントテストへ移し、算術式の優先順位・結合方向を扱う統合テストを追加する。
