# DEPENDENCY

**Understand your code. See how everything connects.**

Локальный desktop-инструмент для исследования собственных программных проектов: структура, зависимости, архитектурные проблемы и возможное влияние изменений.

**1.0.0 Open Beta · Windows x64.** Проект развивается под рабочий процесс владельца. Анализ статический: неизвестные связи отмечаются явно, а не выдаются за подтверждённые.

## Возможности

| Возможность | Содержание |
| --- | --- |
| Анализ | JS/JSX, TS/TSX, базовая поддержка Rust и Python |
| Граф | Обзорная карта каталогов с раскрытием, основания связей, поиск, фильтры и окружение файла |
| Снимки | SQLite, история, восстановление между запусками, AST-кэш |
| Исследования | Заметка и граф на выбранном снимке, выводы Impact, экспорт/импорт со снимком |
| Диагностики | Циклы, связанность и ошибки разрешения с evidence и coverage |
| Impact | Прямые и транзитивные потребители с цепочкой зависимостей |
| Git | Локальные статусы, анализ коммита без checkout, сравнение снимков |
| Доступ агента | Opt-in CLI/MCP: состояние UI, геометрия и чтение сохранённых результатов |

Исследуемый код — данные. Его скрипты, hooks, build.rs, setup.py и макросы не запускаются ради анализа. Git transport отключён; зависимости исследуемого проекта не устанавливаются.

## Установка

Скачайте Windows x64 EXE со страницы [GitHub Releases](https://github.com/f1sherFM/DEPENDENCY/releases/tag/v1.0.0). Это Open Beta: известные ограничения и проверки перечислены в [отчёте выпуска](docs/version-1.0-validation.md).

Закройте приложение перед обновлением. Для резервной копии при закрытом приложении сохраните каталог `%APPDATA%\com.f1sherfm.dependency` целиком. Установщик не удаляет рабочую базу. Импорт исследования не заменяет последний анализ проекта.

## Запуск из исходников на Windows

Нужны Node.js 24, Rust MSVC, Visual Studio C++ Build Tools с Windows SDK и WebView2. Проверки воспроизводятся на Rust 1.95.0. Cargo.toml декларирует минимум Rust 1.90; отдельного прогона на минимальной версии пока нет.

```powershell
git clone https://github.com/f1sherFM/DEPENDENCY.git
cd DEPENDENCY
npm ci
npm run tauri dev
```

Открой папку проекта в приложении. Для анализа локального коммита используй раздел «Git и сравнение». Сохранённые снимки доступны без повторного анализа.

Данные хранятся в `%APPDATA%\com.f1sherfm.dependency\dependency.sqlite3`. Не удаляй базу для обычного обновления. Она содержит пути и факты импортов исследуемых проектов; не публикуй её вместе с отчётом об ошибке.

## Проверки и сборка

```powershell
npm run check:repo
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

Установщики Windows:

```powershell
npm run tauri build
```

EXE/MSI появятся в `src-tauri/target/release/bundle/`. Автоматическая публикация релизов не настроена. Номер версии исходников не означает наличие опубликованного установщика.

CI повторяет основные проверки на Windows. Нативные UI-сценарии проверяются отдельно; зелёный CI не подтверждает все пользовательские действия.

## Границы анализа

- Rust macros/cfg и полное разрешение символов, Python runtime и динамические импорты не вычисляются. Import name не сопоставляется с PyPI distribution наугад.
- Git-снимки используют repository-only профиль: текущие node_modules/venv и конфиги вне выбранного scope не подмешиваются в исторический граф.
- Рабочий снимок использует фактические файлы, а не staged/index; двойная сверка не делает файловую систему атомарной.
- Достижимость в графе не доказывает поломку изменения. Лимиты и пробелы покрытия показываются явно.

Подробности: [0.8](docs/version-0.8-validation.md), [Git и хранение](docs/version-0.7-validation.md), [Impact](docs/version-0.6-validation.md).

## Документация

- [Сохранённые исследования](docs/investigations.md)

- [Локальный CLI/MCP для наблюдения за приложением](docs/agent-access.md)

- [Разработка](docs/development.md) · [правила изменений](CONTRIBUTING.md)
- [Changelog](CHANGELOG.md) · [roadmap](docs/roadmap.md)
- [Концепция](docs/product-spec.md) · [архитектурная основа](docs/architecture.md)
- [Подробная история возможностей](docs/feature-history.md)
- [MIT license](LICENSE)

Версия 1.0 — Open Beta, а не гарантия полного разрешения всех связей. [Проверки и ограничения](docs/version-1.0-validation.md) · [поддержка Python](docs/python-resolution.md).
