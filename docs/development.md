# Разработка

## Среда

Основная платформа — Windows x64. Нужны Node.js 24/npm, Rust MSVC, Visual Studio Build Tools (Desktop development with C++, Windows SDK) и WebView2. CI использует Rust 1.95.0 с rustfmt/Clippy. MSRV 1.90 в Cargo.toml пока не проверяется отдельным CI job.

Используй `npm ci` и Cargo с `--locked`. Оба lockfile хранятся в Git. Обновление зависимостей — отдельная задача, а не побочный эффект изменения документации.

## Структура

| Путь | Назначение |
| --- | --- |
| `src/` | React UI, граф, Git, Impact и IPC-типы |
| `src-tauri/src/analysis.rs` | Общая оркестрация анализа |
| `js_ts.rs`, `resolver.rs` | JS/TS parser и resolver |
| `language.rs`, `native_languages.rs`, `native_resolver.rs` | Профили, Rust/Python facts и resolution |
| `storage.rs`, `snapshot.rs` | SQLite, версии и snapshot facts |
| `git.rs`, `git_jobs.rs`, `comparison.rs` | Capture, jobs и сравнения |
| `diagnostics.rs`, `impact.rs` | Правила и обход графа |
| `src-tauri/tests/fixtures/` | Маленькие проекты-данные для regressions |
| `tests/` | Node graph tests и optional browser smoke |
| `scripts/check-repository.mjs` | Проверка версий и локальных Markdown-ссылок |
| `.github/` | CI и шаблоны issues/PR |

Rust-файлы без полного пути находятся в src-tauri/src.

## Проверки

```powershell
npm run check:repo
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
git diff --check
```

`check:repo` работает offline: согласованность app version в npm/Cargo/Tauri и lockfiles, лицензия manifest, наличие файлов из локальных Markdown-ссылок. Внешние URL и anchors не проверяются. Это не замена тестированию приложения.

Rust форматируется rustfmt. Общего formatter/linter для TS/CSS пока нет; не смешивай массовое форматирование с другой задачей. `npm run check` проверяет TypeScript; build включает эту проверку.

Три Rust profiling/export теста запускаются явно с `--ignored`; обычный CI их пропускает. Команды и browser smoke: [0.8 validation](version-0.8-validation.md). Git CLI нужен для explicit oracle test, не для runtime Git adapter.

## Версии и данные

- App version: package.json, package-lock.json (корень и packages[""]), Cargo.toml, root package Cargo.lock и tauri.conf.json. При релизе обнови также UI-метку и changelog.
- Parser versions независимы по языкам. Resolver/rules/profile/capture policy меняются при изменении соответствующей семантики.
- Schema меняется только при изменении структуры SQLite; миграция должна быть транзакционной.
- Старые snapshots не получают вымышленных новых метаданных. Несовместимые результаты нельзя объявлять архитектурной регрессией.

Обычные тесты используют временные базы. Native app использует базу в AppData: не удаляй пользовательскую историю ради «чистого» теста. Для UI фиксируй проект, исходное состояние и результаты отдельно от engine tests.

## CI и сборки

[Workflow](../.github/workflows/checks.yml): Windows 2022, Node 24, Rust 1.95.0; repository checks, Node tests/build, Rust fmt/Clippy/tests. Actions закреплены полными SHA, при обновлении сверяй их с upstream.

Запуск — push в main, pull requests и вручную. Permissions — чтение содержимого; secrets, подпись и публикация releases не настроены. Первый удалённый результат появится только после push workflow.

`npm run tauri build` создаёт EXE/MSI в src-tauri/target/release/bundle. Успешная сборка не подтверждает установку, native dialogs и все WebView2-сценарии.
