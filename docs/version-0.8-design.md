# DEPENDENCY 0.8.0 — Rust и Python

Статус: реализован первый ограниченный объём 0.8.0. Фактические границы и проверки: [validation](version-0.8-validation.md). Ниже сохранён исходный согласованный план; уточнения реализации имеют приоритет.

## Уточнения реализации

- Schema 3 не потребовалась: профиль и типизированные факты помещаются в существующие JSON-поля schema 2. Кэш действительно версионируется по записи/языку; общий JS/TS parser version сохранён.
- JS/TS parser вынесен в js_ts.rs, общий dispatch — language.rs, Rust/Python facts — native_languages.rs, их resolver — native_resolver.rs. Полный plugin framework не вводится.
- Cargo packages обнаруживаются по захваченным manifests в scope; это не точная имитация Cargo workspace membership/build selection. Поддерживаются основные lib/bin targets, наследование dependency metadata и прямые path dependencies к известному lib target. Targets tests/examples/benches и сложная target/feature конфигурация пока не моделируются полностью.
- Условные Rust module/use не становятся подтверждёнными рёбрами: сохраняются факты и issues. Python условные импорты имеют отдельный kind. Это безопаснее преждевременного объединения cfg-вариантов.
- Для Python from-import подтверждается базовый модуль/package; атрибуты и потенциальные child modules не выбираются автоматически. Namespace portions не дают синтетических файлов и остаются ограниченным случаем.
- Comparison требует полного совпадения descriptors для подтверждённого сравнения. Частичная совместимость по отдельным языкам отложена; при несовпадении показываются наблюдения с предупреждением.
- Новый профиль виден в интерфейсе, module declarations отделены от import kinds и не участвуют в SCC/coupling. В Impact пользователь может отдельно включать структурные и условные связи.

## Цель

Открыть смешанный репозиторий JS/TS + Rust + Python, получить проверяемые статические связи, сохранить результат и сравнить его с другим снимком. Не обещать эквивалентность компилятору Rust или runtime Python. Никакой установки зависимостей, исполнения исходников, build scripts, procedural macros, setup.py или import hooks.

## Основа и необходимые изменения

Существующие capture, SQLite, jobs, graph, diagnostics и Impact сохраняются. `analysis.rs` сейчас совмещает обход, JS/TS AST, npm discovery и resolution. Выделить только языковые операции: обнаружение единиц анализа, извлечение фактов, разрешение запросов. Общая оркестрация владеет inventory, отменой, кэшем и публикацией. JS/TS становится первым адаптером без переписывания работающего resolver.

Tree-sitter уже используется в проекте; для новых языков предлагаются соответствующие грамматики. До выбора конкретных версий проверить совместимость с текущим Tree-sitter, Rust 1.90 и Windows release build. Для TOML нужен data-only parser; конкретный crate выбрать при реализации после проверки зависимостей. Plugin framework, отдельные процессы и Python interpreter не нужны.

Текущего `CachedImport {specifier, kind, line}` недостаточно. Python нужны relative level и imported names, Rust — module context, use tree и attributes. Ввести версионируемые языковые parse facts; контекст проекта не включать в чистый AST-кэш. Один файл может участвовать в нескольких Rust targets: кэш разбора общий, результаты resolution контекстные.

## Общая модель достоверности

- Ребро означает подтверждённую структурную связь в сохранённом статическом профиле, а не доказанное выполнение или использование символа.
- Evidence содержит язык, исходный запрос/строку, единицу анализа, конфигурационный контекст и основание выбора файла.
- Различать dependency import, module declaration и manifest dependency. Связи принадлежности и декларации пакетов не включать молча в прежние SCC/fan-in/fan-out.
- Неоднозначные запросы остаются issues с кандидатами, без выбранного наугад ребра. Условные связи отмечаются отдельно; UI и Impact показывают выбранную политику их включения.
- Coverage хранится по языку/единице и возможности анализатора. Старые снимки не получают вымышленных языковых метаданных.
- Идентификатор внешнего пакета включает ecosystem и контекст manifest, где он нужен. Одинаковые имена npm/PyPI/Cargo не объединяются. Объявленный диапазон версии не равен установленной версии.

## Rust: первый законченный объём

1. Читать Cargo.toml: packages, workspace members/exclude, workspace inheritance, dependencies/dev/build dependencies, переименованные и path dependencies. Учитывать lib/bin и стандартные targets; неизвестные настройки отмечать. Не запускать cargo metadata/check/fetch даже ради discovery.
2. Строить отдельное дерево модулей каждого target: inline mod, `mod name;`, name.rs/name/mod.rs, literal `#[path]`, crate/self/super. Проверять неоднозначность и границы project scope. Один физический файл может иметь несколько логических module contexts.
3. Разбирать use groups, aliases и pub use. Подтверждать связь с модулем или внешним crate только при однозначном разрешении в поддерживаемой модели. Последний сегмент может быть item, а не файл; поиск longest prefix сам по себе не доказывает полное разрешение. Неразрешённые reexport/glob/shadowing chains помечать, не угадывать.
4. cfg/cfg_attr и target/feature dependencies сохранять как условия. Первый профиль — статическое наблюдение вариантов без утверждения, что они одновременно компилируются. Конфликтующие альтернативы не сливать в подтверждённый target. Циклы, включающие условные связи, показывать отдельно от безусловных.
5. build.rs, proc macros, macro-generated modules, include!/OUT_DIR и полное item/type resolution — вне первой версии. Не трактовать отсутствие разобранных use как отсутствие зависимостей: Rust допускает qualified paths в выражениях; эта граница покрытия видна в снимке.

## Python: первый законченный объём

1. Читать .py: import/from, aliases, relative levels, package initializers. Импорты внутри функций остаются статическими фактами; TYPE_CHECKING/try/условия сохраняют контекст. Динамические importlib/__import__ не исполнять и не выдавать за обычные подтверждённые импорты.
2. Поддержать явные source roots в dependency.config.json, типовые flat/src layouts и обычные packages. Автоопределённые roots показывать пользователю; конкурирующие roots не превращать в скрытый порядок sys.path. Namespace portions объединять только в известном профиле, без выбора случайного совпадения.
3. `from pkg import name`: подтверждать найденный package и необходимые initializer-файлы; name может быть атрибутом. Ребро к pkg/name.py допустимо лишь при однозначном основании в поддерживаемой модели. Star imports не разворачивать в произвольный набор файлов, __all__ не исполнять.
4. Читать статические project.dependencies/optional-dependencies в pyproject.toml и простые записи requirements.txt. Includes ограничить scope, лимитом и защитой от циклов; неизвестные директивы явно отмечать. Dynamic metadata, setup.py и backend-specific настройки не вычислять.
5. Имя импортируемого модуля не равно имени distribution: не угадывать PIL→Pillow и похожие соответствия. Неизвестный внешний import хранить как внешний module request, отдельно от объявленной package dependency. Stdlib классифицировать только по выбранной/известной версии Python, без использования окружения компьютера как истины проекта.
6. Не обходить venv/site-packages, не читать .pth, не модифицировать sys.path, не запускать Python. Отсутствие сведений об окружении — ограничение анализа, а не автоматически ошибка проекта.

## Снимки, кэш и Git

Предлагается schema 3: добавить компактный analysis profile JSON к snapshot metadata. Он описывает версии parser/resolver каждого языка, семантику графа, source roots и политики условий. Сам результат получает те же descriptors. Миграция транзакционная; старые JSON читаются с неизвестными полями профиля по умолчанию, без пересчёта истории.

В существующей parsed_files таблице уже есть версия на запись: использовать язык + parser/facts version и content hash; убрать глобальную JS/TS-фильтрацию чтения/записи кэша. Не инвалидировать JS/TS AST при обновлении Python. Resolution rerun при изменениях manifests/config/features без повторного разбора неизменных исходников. Долговременные факты снимка независимы от изменяемого кэша.

Comparison сравнивает descriptors структурно, не через split одной rules-version строки. Совместимость может быть частичной по языкам; отсутствующее покрытие нового языка не означает появление или исчезновение его архитектуры. Разные kinds/evidence требуют обновления identity logic и тестов removed-import.

Repository-only capture расширяется исключениями .venv/venv/__pycache__/.tox/.mypy_cache/.pytest_cache и получает новую версию policy. Старые снимки остаются доступны с предупреждением о несовместимости. Cargo path dependency или Python root за границами scope не копируются неявно. Исторический анализ использует только захваченные файлы и сохранённые настройки.

## Диагностики и интерфейс

Граф показывает язык, ecosystem, тип связи и условность; фильтры работают по полному снимку до проекции. Обычные файловые IDs сохраняются; context evidence не означает дублирование физических файлов в интерфейсе.

Impact использует явный реестр известных edge semantics вместо JS-only списка. Показывает ограничения выбранных языков/условий. Межъязыковые FFI, Tauri commands и Python/Rust bindings не выводятся по совпадению имён.

SCC/coupling применяются к подходящим dependency edges. No-known-consumers для Rust/Python по умолчанию не включать до обоснованной модели entry points/coverage: mod declarations, frameworks, macros и runtime imports делают прежнюю эвристику ненадёжной. Новые правила качества в объём 0.8 не добавлять.

## Последовательность

1. Зафиксировать fixtures и контракты facts/profile/edge semantics; выделить JS/TS adapter, проверить прежние результаты.
2. Добавить schema/profile и независимое версионирование кэша, backward compatibility и comparison semantics.
3. Rust discovery/module graph, затем ограниченное use resolution и явные coverage gaps.
4. Python roots/import resolution, manifest observations и coverage.
5. Интегрировать graph/Impact/diagnostics и исторические Git snapshots; Windows bundle.

## Критерии готовности

- Fixtures Rust: standalone/workspace, lib+bin, nested/inline/path modules, alias/dev/build/path dependencies, cfg alternatives, conflicting candidates, macro-generated gaps, shared source with different contexts.
- Fixtures Python: flat/src, relative imports, initializers, namespace portions, shadowing/module-vs-package ambiguity, from-attribute versus submodule, TYPE_CHECKING, function imports, dynamic imports и отсутствующее окружение.
- Mixed fixture: одинаковые package names разных ecosystems, несколько source roots, Unicode/case collisions Windows, external path refusals. Ни одна неоднозначность не создаёт придуманного ребра.
- Изменение только manifests/config меняет resolution без повторного AST; обновление одного parser сохраняет cache другого. Migration/write failure сохраняют последний snapshot.
- Старые 0.7 и новые снимки открываются после restart; сравнение не сообщает массовое удаление из-за версии/покрытия. Impact проверяется по каждому виду допустимых связей.
- Sentinel build.rs/setup.py/hooks не запускаются; исторические results независимы от текущих venv/node_modules/target.
- Реальные read-only прогоны: DEPENDENCY как JS/TS+Rust, отдельный Python-проект и mixed fixture. Сверять ожидаемые связи вручную/fixtures; не запускать инструменты исследуемого проекта ради эталона.
- Полный Rust/Node regression, Clippy, TypeScript, browser smoke и Windows bundle. Опубликовать проверенные возможности и ограничения, а не только число распознанных файлов.

## Основания

- [Rust modules / path](https://doc.rust-lang.org/reference/items/modules.html)
- [Rust use declarations](https://doc.rust-lang.org/reference/items/use-declarations.html)
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- [Python import system](https://docs.python.org/3/reference/import.html)

Это проектные решения DEPENDENCY на основе документации языков; конкретные библиотеки и поддерживаемые синтаксические границы проверяются до реализации.
