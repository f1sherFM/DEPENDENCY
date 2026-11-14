# Python: статические импорты и bindings

Этап разработки 1.0, приложение ещё не выпущено. Версии: python-parser-3, python-resolver-2; правила diagnostics-6+resolver-7. SQLite schema 3 не меняется.

## Что разрешается

- Обычные/относительные импорты, вложенные regular packages и namespace packages без `__init__.py` в известных source roots.
- `from package import child`: сначала проверяются явные module bindings, затем уникальный подмодуль. Присутствие одноимённого файла само по себе не доказывает зависимость от него.
- Имена функций, классов и простых присваиваний на уровне модуля, import aliases и явные цепочки реэкспорта. Содержимое функции/класса не считается module namespace.
- Regular package ограничивает дальнейший поиск своим каталогом; namespace portions объединяются до обнаружения regular package/module. Несколько regular candidates остаются ambiguous, порядок roots не выдумывается.
- Автоматические module attributes вроде `__name__`/`__path__` не превращаются в зависимости от одноимённых файлов.

Связи остаются файловыми. Например, для `entry.py: from pkg import Public`, `pkg/__init__.py: from .api import Service as Public` и класса Service в `pkg/api.py` граф содержит entry → initializer → api. Прямого entry → api только из-за реэкспорта не добавляется. При отсутствии binding и наличии уникального `pkg/worker.py`, запрос `from pkg import worker` создаёт файловую связь с worker и сохраняет участие initializers.

Явный import binding из внешнего модуля учитывается как объявленное имя. Это не подтверждение установленного distribution, наличия объекта во внешней библиотеке или успешного исполнения Python. Конкретные внешние файлы по такой записи не придумываются.

Основание семантики: [Python import statement](https://docs.python.org/3.11/reference/simple_stmts.html#import), [packages и namespace packages](https://docs.python.org/3.11/reference/import.html#packages).

## Границы достоверности

Сохраняются объяснения для conditional/multiple bindings, wildcard imports, динамического namespace и `__getattr__`, изменяемого `__path__`, ошибок разбора, реэкспортных циклов и глубины более 64. Эти случаи не создают предполагаемого child-file ребра. Базовая связь с явно импортируемым initializer/module при этом может быть подтверждена.

Это статическая модель объявлений: произвольные побочные эффекты вызовов, monkeypatching, import hooks, sys.modules, окружение/stdlib/distribution mapping и запуск в режиме `__main__` не вычисляются. Tree-sitter не заменяет Python compiler/type checker и не проверяет все семантические ограничения параметров типов.

## Дополнение грамматики Python 3.13

Поддержаны defaults type parameters (PEP 696) в class/def/type declarations, включая bounds/constraints, TypeVarTuple и ParamSpec. Обычные type applications не изменены: `list[T = int]` не принимается. Type aliases получают явный module binding. Исходный текст не переписывается; строки импортов и AST statement byte offsets остаются исходными. Настоящие ошибки синтаксиса продолжают создавать parse errors.

Upstream tree-sitter-python 0.25.0 не содержит этой возможности ([issue #346](https://github.com/tree-sitter/tree-sitter-python/issues/346)), поэтому используется локальный MIT patch с закреплённой базой и Tree-sitter CLI 0.25.10. [Происхождение, генерация и удаление patch](../src-tauri/vendor/tree-sitter-python/README.md). Обычный Cargo build использует готовый C и не требует генератора. Лицензия добавлена в bundle resources. При будущей замене на upstream release обязательны regressions и новая Python parser version.

Дополнительно исправлена классификация native manifest limitations: например, исключённая локальная библиотека теперь является resolution-unsupported, а не parse-error. Её отсутствие в индексе не скрывается, правила обновлены до diagnostics-6. Vendor-код исключён из собственного индекса DEPENDENCY явно в analysis.exclude.

## Кэш и история

Parser version 2 ввёл module bindings и точные позиции import statements; version 3 добавляет грамматику PEP 696 и bindings type aliases. При первом обновлении нужны новые факты Python-файлов; JS/TS и Rust продолжают использовать прежние parser versions. Последующие запуски переиспользуют неизменившиеся Python parses. Изменение одного initializer разбирает только его, разрешение зависимостей пересчитывается по всему текущему индексу.

Позиция statement различает несколько команд на одной строке: собственный binding импортируемого имени не должен создавать ложный reexport cycle или скрывать другое присваивание на той же строке. Старые Python facts без позиции десериализуются, старые snapshot JSON не переписываются. Несовместимые профили остаются различимыми при сравнении.

## Проверки

Шесть Rust regressions в python_tests.rs покрывают aliases, attributes против одноимённых модулей, relative/self reexports, сохранность исходной цепочки, namespace portions/ambiguity, implicit attributes, conditional/wildcard/dynamic cases, statements на одной строке, кэш и запись нового snapshot при сохранении старого JSON. Sentinel-код в initializer не исполняется. Существующий mixed-cache test проверяет одноразовое обновление python-parser-1 без повторного parsing Rust/JS.

Read-only прогоны 2026-11-13, source roots по умолчанию, scripts/hooks не запускались:

| Репозиторий | Исходников | Рёбер всех типов | Неполное покрытие | Внешние запросы |
| --- | --- | --- | --- | --- |
| wemake_PS, a275c5adc8d31424897cb26241fd83aa86863b2a, прежний resolver | 409 | 2817 | 898 | 630 |
| Та же копия, новый resolver | 409 | 3082 | 13 | 630 |
| Та же копия, parser 3 с PEP 696 | 409 | 3082 | 12 | 630 |
| pallets/click, e38188e9e205005d5af5f9e59df51904c984586a | 91 | 518 | 22 | 354 |

После parser 3 в wemake остаются 8 запросов к условным/multiple bindings compatibility-модуля, 3 dynamic imports и 1 unresolved module. Parse-error в tests/fixtures/noqa/noqa313.py устранена поддержкой синтаксиса, а не отключением правила. Это не 12 доказанных ошибок проекта. Внешние запросы не приравниваются к отсутствующим пакетам. Дополнительные 265 рёбер и сокращение общих unsupported records сами по себе не доказывают полноту графа: достоверность поддерживаемых случаев проверяется fixtures, неопределённость сохраняется.

Локальный набор после grammar patch: 76 Rust tests passed, 20 Node tests passed; Clippy с warnings denied, rustfmt и TypeScript/Vite прошли. Отдельно прошли 117 upstream grammar cases с неизменными ожиданиями, highlight/tag tests и новая генерация в пустом каталоге с побайтовым совпадением шести generated files. Проверены положительные/отрицательные случаи, locations импортов, смешанный Rust/JS/Python cache, сохранность старого snapshot после записи результата с настоящей синтаксической ошибкой. Три explicit export/profile tests проходили перед grammar patch; реальные Python профили повторены после изменения. Native installer/WebView для этого этапа не обновлялись.
