# 実装タスク

[ARCHITECTURE.md](ARCHITECTURE.md) を仕様とする。チェック済み項目は実装・検証済み。依存タスクを終えてから着手し、各項目の実装と検証が完了した時点でチェックする。各タスクはコードと対応テストを一緒に追加する単位とする。

## T01: 実行基盤とエラー型

- [x] 実装・検証完了

依存: なし。対象: `src/lib.rs`、`src/parser.rs`、`src/error.rs`。

- 初期テンプレートを置き換え、`Rc` で実行器を共有する `Parser<T>` と `Context`、`Step`、`Failure` を定義する。
- 通常不一致と致命的エラー、`Diagnostic`、`ParseError`、`Span`、`ParseOutput<T>` を定義する。
- `parse` / `parse_prefix`、EOF検査、出力型に `Clone` を要求しないcloneを実装する。

完了条件: テスト用の内部実行器で成功・失敗、末尾検査、消費位置、非Clone出力、同じパーサの再実行を検証できる。入力寿命と出力の `'static` 制約を分離できている。

## T02: 基本パーサ

- [x] 実装・検証完了

依存: T01。対象: `src/primitive.rs`。

- `literal`、`satisfy`、`any_char`、`char`、`char_range`、`one_of`、`end` を実装し、公開する。
- 期待項目と失敗位置を記録する。空文字列への一致を許し、文字消費時のUTF-8境界を維持する。

完了条件: 空入力・EOF・改行・マルチバイト文字、文字クラスの範囲端、空の文字集合、文字列の途中不一致をテストする。

## T03: 合成とPEGの制御

- [x] 実装・検証完了

依存: T02。対象: `src/combinator.rs`、`src/error.rs`。

- `then`、`or`、`star`、`plus`、`optional`、自由関数 `and` / `not` を実装する。
- 選択の失敗診断を最遠位置で併合し、同位置の期待項目を重複除去する。
- 反復の非進行を `NonProgressingRepetition` とし、致命的エラーを全合成で伝播する。

完了条件: 途中失敗の巻戻し、成功済み選択の固定、反復の貪欲性、先読みの非消費、反復初回失敗、空成功の検出をテストする。正常な反復停止やオプションの欠如の診断が漏れない。

## T04: 結果変換と構造化

- [x] 実装・検証完了

依存: T03。対象: `src/transform.rs`。

- `map`、`try_map`、`text`、`spanned`、`ignore`、`then_ignore`、`ignore_then` を実装する。
- `try_map` の失敗を消費範囲・終了位置・所有するメッセージ付きの通常不一致に変換する。
- `text` は成功範囲を複製し、`spanned` は元の値を保持する。

完了条件: struct・enumを返す例、数値範囲外からの別分岐試行、マルチバイト文字のSpan、失敗分岐の出力破棄を検証する。外部副作用が巻き戻らないことをrustdocに明記する。

## T05: ルール構築と所有権

- [x] 実装・検証完了

依存: T04。対象: `src/rule.rs`、`src/parser.rs`、`src/error.rs`。

- `GrammarBuilder`、型付き `Rule<T>`、`declare` / `reference` / `define` / `finish` の具体的シグネチャと構築エラーの返却方法を設計文書に追記する。
- 異なる出力型のセル、一度だけの本体設定、文法所有者の強参照と内部参照の弱参照を実装する。
- 未定義・重複定義・別文法の参照を検証する。合成後も参照元の文法を検証できるよう、必要な参照情報を保持する。
- 構築途中・所有者消滅後の参照実行を明示エラーにする。

完了条件: 非再帰ルールで構築検証と型の対応をテストし、ルートのclone・合成が所有者を維持すること、全ルートの破棄でセル・本体が解放されることを検証する。

## T06: 再帰実行と停止性

- [x] 実装・検証完了

依存: T05。対象: `src/rule.rs`、`src/parser.rs`、`src/error.rs`。

- `recursive` を単一ルール用の簡易APIとして実装する。
- 実行中の `(RuleId, input_offset)` を管理し、直接・間接・先読み経由の左再帰を検出する。異なる文法のルールIDも衝突させない。
- 再帰深さ上限の設定APIと保守的な既定値を決定し、文書化・実装する。
- 成功・失敗のどちらでも実行中ルールの記録と深さを復元する。

完了条件: 入れ子の括弧、右再帰、相互再帰、左再帰、上限超過を検証する。致命的エラーが `or` / `star` / `plus` / `optional` / `and` / `not` で隠れないこと、再実行時に状態が残らないことを確認する。

## T07: 利用者向け診断

- [x] 実装・検証完了

依存: T06。対象: `src/error.rs`、`src/rule.rs`、`src/parser.rs`。

- `label`、ルール名の文脈、変換失敗の表示、入力を使った行・列表示を整備する。
- 入力を所有しないエラーに対して、表示時に入力を渡すAPIを具体化して文書に追記する。
- 位置・期待項目の併合、先読みの診断、成功分岐や反復終了時の診断破棄を総合検証する。

完了条件: 最遠位置と同位置併合、Unicodeの列、改行・EOF、ルール文脈をテストする。表示順序が安定し、通常不一致と文法・実行エラーを区別できる。

## T08: 公開例と初版の仕上げ

- [x] 実装・検証完了

依存: T07。対象: `src/lib.rs`、各公開APIのrustdoc、`tests/`、`examples/`、設計文書。

- 設計文書の例をコンパイル可能なドキュメントテストにする。
- 代入文のstruct出力、再帰的enum出力、算術式の優先順位と左foldによる左結合を公開APIの統合テストにする。
- 実行できる算術式の利用例を追加し、全基本式・変数への束縛・再利用が利用者側から使えることを確認する。
- 設計と実装の差分を解消し、初期テンプレートの説明を更新する。メモ化なし・所有出力・単一スレッドという制限を公開ドキュメントに記載する。

完了条件: `cargo fmt --check`、`cargo test`、`cargo clippy --all-targets -- -D warnings` が成功し、利用例を実行できる。設計の検証計画に未検証項目がないことを確認する。


## 完了時の検証記録

全8タスクを実装済み。`cargo fmt --check`、`cargo test`（単体27件・統合6件・ドキュメント6件）、`cargo clippy --all-targets -- -D warnings`、`RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` が成功した。算術式の公開例 `cargo run --example arithmetic -- '10 - 2 * (3 + 1)'` は `2` を出力する。

設計9節の検証項目と実装の対応は以下のとおり。

| 検証項目 | 主な検証箇所 |
| --- | --- |
| 全基本式、EOF、空入力、Unicode、消費位置 | `primitive::tests`、`combinator::tests`、`tests/public_api.rs` |
| 選択の巻戻し・確定、反復の貪欲性、先読み・オプション | `combinator::tests` |
| 非進行反復、直接・間接左再帰、致命的エラー伝播 | `combinator::tests`、`rule::recursion_tests` |
| 変数への束縛、clone、非Clone出力、再実行 | `parser::tests`、`tests/public_api.rs` |
| struct・enum、変換失敗、Span、値の破棄 | `transform::tests`、`tests/public_api.rs` |
| 再帰・相互再帰、構築検証、深さ制限、文法ID | `rule::tests`、`rule::recursion_tests` |
| ルート保持中の寿命と全ルート破棄後の本体・保持値の解放 | `rule::tests`、`rule::lifecycle_tests` |
| 診断の最遠位置・同位置併合・不要診断の除外、行列表示 | `combinator::tests`、`error::tests` |
| 全体・部分パース、複数回実行 | `parser::tests`、`tests/public_api.rs` |
| 設計の利用例、算術式の優先順位・左結合 | `src/lib.rs` のドキュメントテスト、`tests/public_api.rs`、`examples/arithmetic.rs` |
