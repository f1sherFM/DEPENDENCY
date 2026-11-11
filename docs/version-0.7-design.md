# DEPENDENCY 0.7.0 — Git Integration и сравнение архитектуры

Статус: реализовано в 0.7.0. Результаты проверок и ограничения: [validation](version-0.7-validation.md).

## Цель и первый объём

Открыть локальный Git-репозиторий, увидеть его состояние, проанализировать выбранный локальный коммит без checkout и сравнить два сохранённых графа. Пользователь должен понимать, что именно анализировалось и почему связь появилась или перестала подтверждаться.

В обязательный объём входят:

1. HEAD/ветка/detached HEAD, staged и unstaged статусы, untracked, конфликты. Unborn HEAD и отсутствие Git обрабатываются явно.
2. Выбор локальной ветки, тега или коммита; разрешение ссылки в конкретный commit OID до начала анализа.
3. Снимки commit tree и фактически прочитанного рабочего каталога с provenance и inventory файлов.
4. Сравнение двух снимков одного проекта: файлы, подтверждённые связи, счётчики и совместимые диагностики; переход в соответствующий граф и Impact Analysis 0.6.

Полный staged/index snapshot, merge-base/three-dot сравнения, fetch/pull/push, GitHub API, checkout, merge, запись index, watcher и символические изменения экспортов в этот объём не входят. Обычные проекты без Git продолжают анализироваться.

## Что требует изменения в существующем коде

- `analysis.rs` обходит живой каталог; resolver, диагностики и конфигурации используют `std::fs`. Для commit tree нужен изолированный источник содержимого.
- `oxc_resolver` 10 имеет FileSystem abstraction, но одним её внедрением задача не решается: остальные компоненты тоже читают файловую систему. Переписывать весь pipeline под VFS без практической необходимости не предлагаем.
- `parsed_files` — изменяемый кэш последнего разбора по project/path, а не история содержимого файлов. Сравнение не должно читать его как данные старого снимка.
- `save_snapshot` выводит project identity из `summary.root_path` и всегда обновляет latest. Временный каталог commit tree не должен становиться новым проектом, а анализ старого коммита — заменять указатель последнего рабочего снимка.
- В снимках есть граф, диагностики и версии, но нет полного inventory, basis или сохранённых импортных фактов всех файлов. Старые снимки нельзя автоматически объявить снимками текущего HEAD.
- Package-рёбра не сохраняют исходный import kind. Сравнивать можно имеющийся уровень npm-связей и деклараций; изменение type-only/runtime у внешнего пакета пока не доказывается.

## Git Adapter

Предпочтение — локальный adapter на `git2`/libgit2: чтение repository/ref/tree/blob, status и diff без shell-команд и checkout. Транспортные возможности отключить; hooks, credential helpers, внешние diff/textconv/filter drivers не вызывать. Наличие Git CLI в PATH не должно быть условием основной функции.

До фиксации версии зависимости провести маленький Windows-прототип: MSRV проекта, сборка без сетевых transport features, linked worktree, Unicode, длинные пути, отсутствие побочных записей при status и чтении blob. Проверить конкретные API и feature flags выбранной версии. Неподдерживаемый object format и недоступные объекты возвращают понятную ошибку; SHA-256 repository support не обещать до проверки библиотеки.

Repository discovery начинается с выбранного project root, не с переменных Git окружения. Сохранить отдельно project root, worktree root, common Git dir и путь выбранного проекта внутри дерева. Для проекта в подкаталоге анализировать ровно этот scope, не всю соседнюю monorepo. Вложенный репозиторий не объединять с родительским неявно; linked worktrees считать разными рабочими проектами с общей Git object database.

Список refs и история коммитов выдаются страницами до 100 записей. Ветка/тег разрешается в commit и tree OID на старте; дальнейшее движение ссылки не меняет запущенный анализ. Remote-tracking ref — только уже имеющаяся локальная ссылка. Не запускать автоматический fetch при shallow/partial clone; отсутствующие объекты отмечать как недоступные.

## Три различаемых состояния

| Представление | Источник | Гарантия |
| --- | --- | --- |
| Git status | HEAD ↔ index и index ↔ worktree | Два независимых статуса на файл, полученные в указанный момент |
| Commit snapshot | Фиксированный tree и raw blob bytes | Immutable содержимое выбранного дерева в scope проекта |
| Worktree snapshot | Зафиксированный набор прочитанных файлов, включая допустимые untracked | Точные проанализированные байты, но не атомарная фотография всей файловой системы |

Staged-статус не означает, что анализировалось staged-содержимое. При одновременных staged и unstaged изменениях рабочий анализ использует фактический файл, а не blob index. Удаления, intent-to-add, type changes и конфликтные index stages показываются отдельно. При конфликтах рабочие файлы могут содержать маркеры; parse/coverage ошибки остаются видимыми.

Git status — изменчивое наблюдение, не свойство давно сохранённого снимка. В 0.7 сохраняются basis, время capture и fingerprints HEAD/index/содержимого, а полная таблица status остаётся отдельным текущим наблюдением со своим временем. Историческому снимку не приписывается текущий status.

## Изолированный вход Analysis Engine

Рекомендованный первый подход — private временный каталог, содержащий только подготовленные входы snapshot. Заполнять его напрямую raw blobs или копиями выбранных worktree-файлов, затем использовать существующие AST/resolver/rules. Это материализация данных, а не `git checkout`, `git archive` или исполнение проекта.

Добавить небольшой `AnalysisInput`: logical project identity/root, materialized root, basis, явный inventory и resolution profile. Engine возвращает только логические относительные пути; временный root не попадает в project identity, diagnostics evidence или сохранённый rootPath. Перечень анализируемых файлов задаётся capture, повторный неконтролируемый filesystem walk не меняет его.

До материализации validate все пути: принадлежность root, отсутствие traversal/absolute paths, специальных Windows device names, ADS, обратных слешей, case-fold collisions и конфликтов file/directory. Проверять реальные границы записи. Непредставимый tree нельзя молча нормализовать в другой набор файлов: анализ завершится объяснимой ошибкой без публикации нового снимка.

В первом объёме symbolic links не создавать и не обходить, submodule/gitlink не клонировать, LFS pointer не подменять текущим содержимым worktree. Такие записи сохраняются в inventory как unsupported/omitted с причиной. Недоступный blob или ошибка копирования прекращают capture; специально поддержанные пропуски делают coverage неполным. Никакие .gitattributes/smudge/clean/textconv команды не выполняются. Commit анализирует raw bytes; окончания строк и LFS могут отличаться от файлов рабочего каталога.

Ограничить capture числом entries, размерами blob/файла и общим объёмом. Начальные ориентиры: 100 000 entries, 10 МиБ на исходный JS/TS, 512 МиБ суммарно; уточнить на fixtures. Достижение ресурcного лимита — явная ошибка, а не правдоподобный усечённый граф. Поддержать адресную отмену Git capture/нового Git-analysis job; уборка только своего проверенного временного каталога.

## Достоверность resolver и ignore-политика

Для сравнимых новых Git-снимков использовать общий профиль `repository-only-v1`: только захваченные файлы/конфиги/workspace manifests, одинаковые условия resolver и предсказуемые исключения generated/dependency каталогов. Не подмешивать текущие node_modules, конфиги предков временного root или внешние абсолютные tsconfig paths. Профиль и его hash сохранять в snapshot.

Это требует проверить и закрыть все внешние filesystem обращения resolver/config readers. Path restrictions resolver сами по себе не доказывают изоляцию чтения: дополнительно контролировать config extends и поиск package metadata. Acceptance fixture с «отравленным» tsconfig/node_modules вне capture должен давать тот же результат, что чистая среда.

Внешний npm import остаётся известной ссылкой на имя пакета, но точность installed-package exports не заявляется без зафиксированного содержимого зависимости. Если оно необходимо для связи, показывать причину неполноты; зависимости не устанавливать. Изменение manifests/aliases всё равно запускает resolution и rules заново поверх переиспользованного AST.

Tracked файлы в scope входят по выбранной общей политике даже при совпадении с .gitignore; generated exclusions учитываются отдельно. Для worktree untracked брать только неигнорируемые файлы. Для commit не применять текущие global ignore и .git/info/exclude. Сохранять inventory включённых и исключённых entries с причиной; изменение capture policy делает structural comparison ограниченным по сопоставимости.

## Capture рабочего каталога

Перед capture записать HEAD/index fingerprint/status; прочитать и сохранить входные bytes/hash в private staging; повторно сверить перечень кандидатов и содержимое/метаданные, HEAD и index. При обнаруженном изменении предложить повтор или выполнить один ограниченный retry. Статус получения — captured/changed-during-capture, не выдуманная метка «атомарно».

Даже двойная проверка не исключает ABA и параллельное изменение между чтениями. Снимок утверждает, какие bytes были проанализированы, а не существование единого момента, в который все эти файлы гарантированно совпадали. Для строго immutable baseline выбирать commit.

Index fingerprint строить из нормализованных entries (path, mode, OID, stage и значимых flags), а не только mtime файла `.git/index`. Include/exclude manifest и SHA-256 захваченных bytes входят в worktree fingerprint. Нулевой HEAD допустим в новом репозитории. Наблюдение index не должно выполнять refresh/write index или fsmonitor hook.

## Provenance и хранение

Добавить в JSON результата nullable `basis` и snapshot `files` inventory. Для включённого файла хранить path, kind/mode, content SHA-256, Git blob OID при наличии, inclusion/coverage status. Для разобранных исходников — извлечённые import facts со specifier/kind/line и parser version. Это facts текущего снимка; не читать их позже из изменяемого parsed_files cache.

```text
SnapshotBasis {
  kind: filesystem | git-worktree | git-commit | legacy-unknown,
  repositoryIdentity, projectScope,
  requestedRef?, commitOid?, treeOid?, observedHeadOid?, branchLabel?,
  indexFingerprint?, captureFingerprint?,
  startedAt, completedAt, captureConsistency,
  resolutionProfile, capturePolicyHash, versions
}
```

Git repository identity — локальная привязка, не remote URL (URL может отсутствовать/меняться). OID не называть SHA-1 в контракте; отдельно фиксировать object format, когда доступно.

Изменить `save_snapshot`: явные projectId/logical root и политика продвижения `latestWorktreeSnapshotId`. Снимок старого коммита сохраняется в том же проекте, но не заменяет последний успешный рабочий анализ. Отдельно можно помнить последний открытый snapshot для восстановления UI. Существующий latest pointer мигрировать как legacy latest без выдуманного Git basis.

Нужна небольшая транзакционная SQLite migration (схема 1 → 2) для указателей и metadata, не полная нормализация графа. Крупные inventory/import facts учитывать в существующем лимите чтения Impact JSON 64 МиБ: отделить графовый payload от file facts (таблица snapshot_files или отдельный snapshot metadata payload), чтобы Git metadata не делали ранее доступный impact недоступным. Конкретный layout выбрать до миграции и измерить размер fixtures.

Предпочтение: `snapshot_files(snapshot_id, relative_path, metadata_json, imports_json)` с FK на snapshots, а графовый `result_json` сохраняет basis/summary, но не дублирует полный inventory. Все вставки files, graph, parser cache, versions и latest pointer фиксируются одной транзакцией. Ошибка миграции/анализа/записи сохраняет прежний готовый snapshot.

Parser cache можно переиспользовать при совпадении content hash и parser version; scope/project идентификация остаётся явной. Пока достаточно существующего кэша по project/path: возврат между коммитами может потребовать повторного parse, но не должен выдавать ошибочный cache hit. Blob OID и SHA-256 исходного содержимого — разные идентификаторы, их нельзя сравнивать напрямую.

## Сравнение двух снимков

Сравнение — `base → target`, по двум snapshot IDs одного project/scope. Это сравнение состояний, не merge-base diff; порядок задаёт пользователь. По умолчанию только готовые снимки с совместимыми parser/resolver, resolution profile и capture policy.

При разных версиях или неполном покрытии показывать причины и доступные факты раздельно. Не объявлять количественные изменения диагностик архитектурным ухудшением при изменённых правилах/thresholds. Legacy снимки без inventory поддерживают только ограниченное сравнение сохранённых графов; приписывать им commit/hash/rename нельзя. Переанализ старых источников — отдельное явное действие, а не мутация истории.

Сопоставление файлов первой реализации:

1. Точный относительный путь с сохранением регистра. Одинаковый путь/другие bytes → modified.
2. Для оставшихся unmatched — совпадение content SHA-256, глобально уникальное среди included файлов каждой стороны, → moved-exact. Не объединять несколько одинаковых boilerplate/copy файлов произвольно.
3. Остальные записи → added/removed или ambiguous. Similarity rename detection Git можно показать отдельно как предложение с источником и порогом, но в обязательном объёме не использовать его для автоматического сопоставления графов.

Case-only перемещение проверять отдельно на Windows; сочетание rename+edit в первом объёме может остаться added/removed. Отсутствие подтверждённого переименования не равно доказательству, что его не было. Глобальные stable IDs и symbol lineage пока не вводить.

Для рёбер сравнивать `(mapped source, mapped target, kind)` и число исходных импортов. Строка и edge.id — evidence, не identity: перенос импорта на другую строку не должен создавать ложное удаление/добавление связи. Package targets сопоставлять по сохранённому имени; диапазон из package.json не выдавать за установленную версию.

Разделить:

- изменение текста/фактов импортов (по file facts),
- изменение подтверждённого resolver target (по графу),
- изменение подтверждаемости связи (неполное coverage/новая ошибка resolution).

Если target snapshot перестал разрешать импорт, карточка говорит «связь больше не подтверждается», а не «зависимость удалена». Исчезнувший из индекса/политики файл тоже не равнозначен удалённому из Git файлу. Метрики считаются по совместимой общей области; при разном покрытии delta сопровождается ограничением.

Диагностики сравнивать лишь при одинаковых rules version и effective config hash. Diagnostic.id 0.5 включает evidence, поэтому не считать его межснимочной идентичностью: использовать ruleId и надёжно сопоставленные субъектные узлы, отдельно изменения evidence/порогов. При неоднозначности показывать карточки обеих сторон без выдуманного «исправлено».

## Impact и UI

В проекте — Git-контекст и две независимые колонки staged/unstaged; отдельные untracked/conflicted. В истории — basis, commit/ветка на момент capture, версии и профиль анализа. Commit picker показывает только локальную историю.

Сравнение имеет явные base/target, сводку coverage/совместимости, списки файлов/связей/диагностик и доступ к evidence каждой стороны. До публикации commit snapshot UI показывает выбранный immutable OID, scope, профиль resolution и лимиты; имя ветки не заменяет OID.

Для изменённых/удалённых файлов предложить Impact Analysis в base snapshot, для добавленных/изменённых — в target. Два результата показывать раздельно с их версиями и полнотой. Не переносить node IDs между снимками и не объединять counts через неоднозначное file matching.

Списки refs/status/inventory/diff — страницы до 200, граф сохраняет лимиты 0.4. Новый Git capture/analysis выполняется фоновым адресным job с прогрессом и cooperative cancel между чтениями, файлами и стадиями. Устаревший ответ не заменяет выбранный пользователем проект/снимок. Сравнения в первой реализации можно кэшировать ограниченно в памяти по base/target/version/options; отдельная долговременная таблица comparisons пока не обязательна.

## Этапы и критерии проверки

1. Git adapter prototype на Windows. Фикстуры: normal/unborn/detached repository, linked worktree, nested repository, Unicode, shallow/missing object, unsupported object format. Проверить отсутствие изменений HEAD/index/refs и запуска hooks, filters, textconv, fsmonitor или сетевых операций.
2. Basis/inventory, migration и явная save policy. Проверить legacy defaults, rollback миграции/записи, сохранение latest рабочего снимка после анализа старого коммита и Impact 0.6 на старых snapshots.
3. Изолированный capture и общий repository-only resolver. Фикстуры: разные aliases/exports в двух коммитах, untracked/ignored/staged+unstaged, подкаталог monorepo; враждебные внешние конфиги/node_modules. Проверить независимость commit результата от живого worktree.
4. Capture races, conflicts, symlinks, submodules, LFS pointer, traversal, Windows collisions/reserved names и ресурсные лимиты. Временные пути не сохраняются как project root; cancel не публикует snapshot и не оставляет свои staging-каталоги.
5. Comparisons: move exact, rename+edit, одинаковые copies, case-only rename, сдвиг строк, изменение только tsconfig/manifest, unresolved target, исключённый файл, разные правила/thresholds и legacy coverage. Проверять реальные рёбра обеих сторон.
6. UI smoke и Windows bundle. На нескольких локальных Git-репозиториях сверить status и выбранные commit trees с Git CLI как независимым read-only oracle в тестовом окружении; source hooks не исполнять. Измерить capture/analysis/diff, размер inventory и память.

Готовность: пользователь может безопасно сохранить архитектуру локального коммита и рабочего содержимого, объяснимо сравнить их и исследовать последствия в каждом snapshot. Результат воспроизводим по сохранённым фактам и версиям; текущий worktree не подменяет историческую архитектуру.

## Источники для технических решений

- [git2 Repository API](https://docs.rs/git2/0.21.0/git2/struct.Repository.html) — локальные refs, tree/blob и status APIs; зафиксирована ветка 0.21 без default features, libgit2 1.9.7.
- [libgit2 raw blob content](https://libgit2.org/docs/reference/main/blob/git_blob_rawcontent.html) — чтение исходного содержимого blob без checkout.
- [Git status](https://git-scm.com/docs/git-status) — различие index/worktree состояний и untracked.
- [Git diff](https://git-scm.com/docs/git-diff) — разные базы сравнения и similarity-based rename detection.
