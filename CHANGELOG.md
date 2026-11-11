# История изменений

Версии обозначают этапы разработки. Даты релизов не восстанавливаются по предположениям; tags/releases не создаются автоматически. [Подробная история](docs/feature-history.md).

## Unreleased

- Оформление README, руководство разработки, roadmap, contribution guide и MIT license file согласно Cargo metadata.
- Windows CI и offline-проверка версий/ссылок документации.
- Функции 0.9 пока не реализуются.

## 0.8.0

- Статический Rust/Python анализ в общем pipeline, независимые языковые кэши, факты и профили снимков.
- Новые типы связей в графе/Impact, декларации Cargo/PyPI.
- [Проверки и ограничения](docs/version-0.8-validation.md).

## 0.7.0

- Локальные Git status/refs/commit analysis, background jobs и отмена.
- Immutable inventory, SQLite schema 2, сравнение снимков.
- [Проверки](docs/version-0.7-validation.md).

## 0.6.0

- Impact по сохранённому снимку: reverse traversal, цепочки, фильтры и бюджеты.
- [Проверки](docs/version-0.6-validation.md).

## 0.5.0

- Архитектурные диагностики с evidence, настройками и coverage.
- [Проверки](docs/version-0.5-validation.md).

## 0.4.0

- Интерактивный граф: каталоги, фильтры, навигация, pan/zoom, явные лимиты.

## 0.3.0

- JS/TS resolver: aliases/baseUrl, exports/imports, workspace packages.

## 0.2.0

- SQLite, атомарные снимки и повторное использование AST.

## 0.1.0

- Первый JS/TS AST-анализатор и файловый граф.
