# Проверка перед 0.9.0

Дата: 2026-11-12. Итоговая сборка — 0.9.0, включая сохранённые исследования. Ниже сохранены результаты промежуточных проверок на 0.8.0; актуальный итог приведён в этом разделе. Это отчёт о выполненных проверках, а не утверждение об отсутствии багов.

## Результат

- Предыдущий опубликованный CI для оформления репозитория: [успешный run](https://github.com/f1sherFM/DEPENDENCY/actions/runs/43078251356), commit 6d240a8. Он не проверяет изменения 0.9; результаты нового запуска после push следует смотреть в GitHub Actions. Ниже перечислены локально выполненные проверки этой версии.
- Локально: 66 Rust tests passed, 3 explicit profiling/export tests прошли отдельным запуском, 17 Node tests passed (граф, MCP и сводка покрытия); rustfmt, Clippy с warnings denied, TypeScript/Vite и repository checks прошли.
- Финальный browser smoke в Chrome прошёл на Git fixture, Impact fixture и настоящем сохранённом снимке с 637 случаями ограничений. Tauri transport/job timing в браузере mocked; native engine/storage/jobs проверяются Rust-тестами.
- Собраны MSI и NSIS 0.9.0; NSIS /S установлен с exit 0. Установленный executable SHA256: E5924A0F435A6CBF9A3CF5BBB592D3E841BA880B19A32AA20DE3BA0E816503BE. UI подтверждает v0.9.0.
- Перед миграцией при закрытом приложении сохранена резервная копия в AppData/com.f1sherfm.dependency/backups/pre-0.9-39d24b5b-60a1-447a-b06f-c6df64a92d2c. После запуска schema 2 → 3: контрольные суммы всех 14 snapshots, 1992 snapshot_files, 495 parsed_files и идентичности/указателей трёх проектов совпали. quick_check=ok, foreign_key_check пуст. Старое приложение 0.8 не открывает schema 3; откат требует этой резервной копии.
- Native подтверждены открытие picker через карточку проекта, отмена без смены snapshot, сохранение исследования, полное закрытие приложения и восстановление исследования после холодного запуска. Проверено на установленной 0.9.0 через Computer Use; MCP использовался только для чтения результата. Coordinate input работает в обычном окне, но в maximized иногда возвращает неверные bounds 14×14; для продолжения использовались клавиатура и Restore через системное меню.

### Сохранённые исследования

Имя, заметка, filters/focus/selected node, контекстная панель и камера сохраняются в отдельной таблице со ссылкой на неизменяемый snapshot. Камера хранит мировой центр и масштаб для другого размера окна. Формат состояния имеет собственную версию; update использует revision, конфликт не затирает предыдущую запись. Повторного анализа и неявного переноса на latest нет. Подробности — в [контракте исследований](investigations.md).

Четыре Rust-теста проверяют reopen, удалённые исходники, исторический snapshot, concurrent writers, rollback ошибки записи, FK, неизвестный формат и неизменность snapshot/cache. Browser smoke во всех трёх прогонах проверил создание, reload/resize, восстановление snapshot/filter/selected/camera, редактирование заметки, подтверждённое удаление и отсутствие analysis IPC.

Исправлена гонка browser harness: ожидание selector после Page.reload могло увидеть старый документ. Теперь сначала ожидается Page.loadEventFired, затем готовность нужного интерфейса. Финальные три прогона прошли после исправления.

Native сохранено контрольное исследование «Проверка сохранения 0.9» (a1947378-5c5b-4347-82f9-4dba12cda185) на снимке 7aea8f56-d70a-4062-925b-6e2143e4abf8. После Alt+F4 процесс завершился; запущена установленная копия, через список открыта прежняя запись. Заметка и directories mode восстановились. При canvas 672.4×225.8 transform translate(20,-6.796875) scale(1) соответствует сохранённому мировому центру (316.1953125,119.6953125) с погрешностью менее 0.01. После разворачивания окна zoom остался 1, header/root scroll и footer находятся в границах viewport; runtimeErrors пуст. Все контрольные суммы snapshots/inventory/cache/project pointers совпадают с состоянием до обновления. Новых анализов не создавалось. Пример оставлен в приложении для проверки пользователем.

## Исправления

### Сводка 637 сообщений покрытия

По readonly MCP/CLI прочитан сохранённый снимок 99159959-638c-4bfa-be99-332cee2bed7e: 637 записей полностью относятся к resolution-unsupported — 545 macro expansion, 71 item/reexport/external-crate context, 19 conditional use и 2 conditional module. Это четыре типа ограничений, а не 637 ошибок проекта. Отдельных unresolved/ambiguous imports и parse failures в этом снимке не зафиксировано. Ограничения не доказывают полноту графа и не были удалены.

Problems показывает количество типов/случаев/файлов с русскими названиями; отдельно отображаются unresolved, ambiguous, parse-error files, capture omissions и дополнительные записи неизвестного типа. Подробности методики/сырых counters свёрнуты в details, каждая диагностическая запись и evidence доступны как раньше. Summary строится по summary.resolutionIssues, parseErrorCount, basis.omittedCount и rulesRun.coverageIssueCount, независимо от фильтров и выключенных rules; нерасшифрованный остаток не приравнивается к нулю ошибок. Неизвестные причины сохраняются исходным текстом. Impact использует нейтральную формулировку «записи о неполном покрытии».

Добавлены 3 Node tests: 637→4 без diagnostics, раздельный учёт ошибок/capture/unknown records, сохранность всех ID/evidence при группировке. Прошли 17 Node tests, 62 Rust tests и Clippy. Browser smoke адаптирован для реальных снимков без архитектурных находок; прогон на настоящем сохранённом 637-case JSON прошёл, скриншот сводки просмотрен. Отдельно прошёл Git fixture smoke. Данные снимка экспортировались read-only, анализ исследуемого проекта не перезапускался.

После одного CDP timeout при повторном прогоне большого JSON тестовый harness изменён: перед установкой нового IPC fixture старый Page.addScriptToEvaluateOnNewDocument удаляется, скрипты и listeners не накапливаются. После изменения прошли real-637 и Impact прогоны. Финальные MSI/NSIS собраны и NSIS установлен с exit 0; hash 74C78E75BA37513F15281D4ED18DF6F724FC8F8A8783A929A98B2122007899C3. В настоящем приложении клавиатурой открыт Problems; канал наблюдения подтвердил тот же snapshot 99159959-638c-4bfa-be99-332cee2bed7e, «Типов ограничений анализатора: 4 · Отмеченных случаев: 637», все четыре русских описания, отдельные нулевые счётчики unresolved/ambiguous/parse errors, отсутствие прежней формулировки и runtimeErrors=[]. Приложение оставлено на этой странице; новые snapshots не создавались.

### Сброс масштаба и «Вписать граф»

Пользователь сообщил порог около 64% и неработающее вписывание. Само число 64 не является заданным ограничением: предел ручного zoom — 2%. В базовом browser-сценарии порог 64% не повторился; отдельный regression воспроизвёл подтверждённую причину сброса: после ручного zoom до 0.11215666 resize canvas возвращал initial camera с zoom=1. Один effect безусловно выставлял initial camera на любое изменение viewport, в том числе reflow панели coverage.

Камера теперь различает initial/manual/fit. Только изменение самой раскладки запускает читаемую initial camera. Resize сохраняет ручные zoom/pan, а после явного fit пересчитывает именно fit. ResizeObserver игнорирует нулевые и неизменившиеся размеры. Browser regression на 9-node star проверяет 12 последовательных уменьшений ниже 64%, сохранение масштаба при resize, размещение всех nodes после fit, устойчивость fit после observer callbacks и последующего resize, реальный background drag через CDP, сохранение pan при reflow, wheel zoom и достижение 2%. Сценарий до исправления падал на 0.11215666 → 1; после исправления прошёл. TypeScript/Vite и 14 Node tests прошли.

Git/Impact browser smoke и Windows MSI/NSIS build прошли. Установленный бинарник обновлён (hash CF940F7C2587A13C2266A794F472B959D6E3D5C5F7038A9A1C3A28EDB1265968). Native проверка клавиатурой: 5 нажатий «Отдалить» дали zoom=0.40187757 (40%), затем «Вписать граф» дало zoom=0.95 (95%). Канал наблюдения подтвердил актуальные transform, 5 из 5 nodes в области canvas, stale=false и runtimeErrors=[]; скриншоты соответствуют. Это проверка настоящего WebView, а не только headless browser. Пользовательские snapshots не пересканировались и не изменялись.

### Канал наблюдения и MCP

Добавлен opt-in файловый канал состояния реального UI и MCP/CLI-адаптер. Backend атомарно публикует observation через tempfile.persist в AppData, запись выполняется вне UI thread. Frontend сообщает точный текущий snapshot/project/view/selection, фильтры, геометрию и ID графа, rendered text с лимитом, error banner и последние 20 runtime errors. Heartbeat 3 секунды; по умолчанию отключён, отзыв доступа действует без перезапуска. Node-адаптер читает SQLite с readOnly/query_only, без вызова анализатора и без изменения last-opened/latest. Неподдерживаемая schema отклоняется, возраст/PID определяют stale, несколько живых сессий не выбираются неявно. [Контракт и подключение](agent-access.md).

62 обычных Rust tests и 14 Node tests прошли. Новые проверки: атомарная замена/лимиты/opt-in, неизменность SQLite bytes после чтения, исторический UI snapshot вместо latest, отказ смешивать selected node с другим снимком, pagination доказательств, stale/multiple sessions, отключение живого адаптера, неизвестная schema. Официальный MCP SDK client выполнил initialize, tools/list и tools/call через настоящий stdio server. Browser smoke на Git/Impact fixtures сверяет опубликованные node IDs/snapshot/viewport с DOM; дополнительный ErrorEvent проверяет передачу текста и source location. Это транспортная проверка, а не утверждение о полном отсутствии runtime bugs.

Сервер dependency зарегистрирован через codex mcp add с абсолютным Node/script/AppData. Предыдущая Codex config сохранена в Temp/dependency-codex-config-b26775d3-5948-4870-9cc6-c2e90905feb1.toml. Существующие MCP entries сохранены. В текущем чате каталог новых MCP tools не обновился автоматически; CLI доступен без перезапуска чата.

Native проверка: финальная MSI/NSIS сборка установлена (exit 0), installed hash 5966FC9181AE0B33C68E8F5E343E77C0A1D69AB48027B646E70AA96FF7721B4B. CLI прочитал живой overview Desktop-проекта: snapshot 99159959-638c-4bfa-be99-332cee2bed7e, 31 source, 48 internal dependencies, 0 architecture warnings, 637 coverage warnings, viewport 1536×890/DPR 1.25, runtimeErrors []. Затем граф открыт клавиатурой через Computer Use; официальный SDK client подключился к настоящему stdio server и прочитал 9 tools, view=graph, тот же snapshot, age=1570ms/stale=false, 5 rendered/on-screen nodes, реальные filters и canvas height=420px. Footer bottom=842.8 при viewport height=890, rootScrollTop=0. Индикатор «Агент · чтение» виден в native UI. Это проверка установленного приложения и его настоящей SQLite, без browser mock и без повторного анализа через MCP.

### Размер окна и цикл парсера

Пользователь обнаружил глобальную прокрутку, уводившую верхнюю панель за границы окна, и один реальный статический цикл analysis.rs ↔ js_ts.rs. Оболочка теперь имеет фиксированную высоту viewport и grid row minmax(0,1fr); sidebar, main и context прокручиваются независимо. Граф занимает оставшуюся высоту вместо собственного расчёта от 100vh. Метаданные источника снимка свёрнуты в details; в низком окне заголовок компактнее. При смене раздела/snapshot main scroll сбрасывается. Узкая context panel открывается поверх main и доступна для закрытия. Tauri стартует maximized, minimum window 800×560 вместо 1080×700.

Browser regression на effective CSS viewports 800×560, 1024×640, 1280×720, 1536×864 и 1920×1080 при deviceScaleFactor 1.25 проверяет длинный список из 125 находок: scroll остаётся внутри main, header видим, нет горизонтального overflow оболочки. Для графа проверяются доступная область рисования, footer controls в пределах main, сброс scroll при навигации и открытие/закрытие контекста в узком окне. Первый прогон обнаружил лишь 87px canvas при 800×560; после уплотнения заголовка полный набор прошёл. Снимки каждого размера просмотрены выборочно; это браузерная проверка, не изменение Windows DPI.

ImportDecl принадлежал analysis.rs, но использовался парсером, вызываемым самим analysis. ImportDecl и сериализуемый Fact перенесены в независимый parse_facts.rs, оба парсера используют его. Формат JSON/cache не менялся, версии парсеров не повышались. Self-analysis regression проверяет подтверждённые рёбра parser → parse_facts и отсутствие parser → analysis. Fixtures с настоящими циклами продолжают проверять диагностику; правило не отключалось. Новый read-only анализ workspace: 31 source, 129 edges всех типов, архитектурных циклов не найдено; ограничения покрытия остаются.

В Desktop-копии применено только такое же устранение цикла в исходниках. До записи analysis/js_ts/native_languages/lib сверены с её HEAD, native_resolver — с прежним авторским QA patch; предыдущие пять файлов сохранены в Temp/dependency-desktop-cycle-backup-oi45ai31. Это целевой patch копии для анализа, а не полная синхронизация workspace. Исторические снимки с циклом должны сохранять старую находку до нового анализа.

Повторный read-only прогон Desktop-копии: 31 source, 129 edges всех типов, циклов не обнаружено, 637 coverage occurrences сохранены. Финальная сборка установлена NSIS exit 0, binary hash E7CBB2015ACD49169BD6B6227CE1F295B2A4671C519673F13A82E5F4CCC86387; при запуске native window развёрнуто, header находится в пределах окна, scroll принадлежит main. Через Computer Use с Shift+Tab визуально выбрана кнопка «Запустить анализ», Return запустил native Git-analysis job; это работающая клавиатурная навигация, отдельная от блокера mouse input.

Native job завершился успешно: в установленном приложении на Desktop-копии отображаются 31 source, 48 internal dependencies (без структурных/пакетных рёбер), 30 packages, 0 cycles и навигационный счётчик «Проблемы 0». Старый снимок с 1 cycle сохранён в истории. Это подтверждённый запуск и пересканирование через клавиатуру реального WebView, не только browser mock.

### Результаты ручной проверки пользователя

На скриншотах 647 warnings смешивали 3 архитектурные находки и 644 ограничения покрытия. Навигационный счётчик теперь относится только к архитектурным предупреждениям; покрытие доступно отдельной кнопкой и фильтром. Одинаковые сообщения группируются по rule/severity/explanation, раскрываются порциями по 30 мест без потери сохранённых доказательств. Это представление данных, а не расширение поддержки Rust macros. Старые снимки не пересчитываются.

Встроенные `node:test`, `node:test/reporters`, `node:sea`, `node:sqlite` больше не создают npm nodes; bare `test`/`sqlite` остаются возможными пакетами. Общие ограничения Rust/Python не выдаются за parse errors: они сохранены в профиле, no-known-consumers явно пропускается для native-языков, а Impact сохраняет known-gaps при ограничениях профиля даже без индивидуальных issues. Диагностическая версия повышена до diagnostics-5+resolver-5, JS resolver profile — до js-ts-resolver-4. Parser versions прежние.

`analysis.exclude` в dependency.config.json исключает исходники и манифесты из индекса; шаблоны сохранены в effective config и analysis profile. Собственный конфиг исключает только src-tauri/tests/fixtures/**, обычные тесты остаются. Regression проверяет удаление fixture cycles/packages, отсутствие повторного AST parsing и отказ при невалидном glob. Разный scope делает сравнения несовместимыми.

Граф по умолчанию скрывает external packages. Автоматическая камера сохраняет масштаб не менее 100%, уведомляет о содержимом вне экрана; полное вписывание остаётся явной кнопкой. Browser regression добавляет 50 пакетов и проверяет оба режима. На 644 synthetic coverage occurrences проверены одна свёрнутая группа, порционное раскрытие и независимый nav count. Browser smoke прошёл на Git, Impact и свежих реальных данных самого DEPENDENCY. Один прогон с большим JSON завершился CDP timeout; повторный завершился успешно.

Свежий read-only self-analysis: 30 sources, 126 edges всех типов, 1 архитектурный цикл, 658 отдельных resolution-unsupported occurrences (в основном Rust macros). Снижение числа sources с 81 объясняется явным исключением fixtures, а не потерей файлов. Mixed fixture отдельно: 8 sources, 10 edges; сохранены ограничения профиля. Native Computer Use смог прочитать реальный граф и UIA установленного приложения; стабильность ввода пока не подтверждена.

Финальные MSI/NSIS собраны. NSIS silent upgrade завершился с exit 0, installed binary hash изменился на 9E860A79A1274287C5C2316DFB746D7D5E1D475A378F2C21256621B45D4FB6DD. В Desktop/DEPENDENCY добавлен только отсутствовавший dependency.config.json; исходники копии не перезаписывались. Через Computer Use прочитано окно установленной сборки: Desktop project, 30 sources, 46 internal dependencies, 30 packages, 1 architectural warning, snapshot 2026-11-12 16:43:05. Native click по разделу Problems снова отклонён инструментом: window bounds 14×14. Это не успешный native click-through; browser regression и фактический запуск подтверждены отдельно.

### Карточка проекта в sidebar

После пользовательской проверки обнаружено, что верхняя карточка проекта выглядела как кнопка, но была div без обработчика. Native picker вызывался только нижней quick action. Карточка заменена на semantic button, использует общий scan handler, блокируется при busy и доступна с клавиатуры. Browser regression нажимает именно карточку, проверяет отказ picker, затем отмену через quick action и успешную передачу выбранного пути в open_project. Это не подменяет оставшуюся native-проверку диалога.

### Обрезание preview графа

Overview размещал до 36 узлов за границами SVG viewBox: правый столбец достигал x=743 при ширине 720, нижний ряд y=283 при высоте 260. Regression с 36 узлами сначала упал; viewBox расширен до 780×310. Browser проверяет геометрические границы всех узлов.

Клик preview-узла раньше менял только скрытый selected state: overview не открывал граф. Теперь он переводит в полный граф с выбранным узлом; сценарий закреплён browser test. Нереализованная кнопка настроек отключена с явной подписью вместо доступного действия без обработчика.

### Ошибка системного выбора папки

В App.scan вызов native folder picker находился перед try/catch. Ошибка plugin IPC могла стать необработанным Promise rejection без сообщения в интерфейсе. Busy также устанавливался только после закрытия диалога.

Выбор папки перенесён внутрь общего try/finally; busy устанавливается до открытия и снимается при отмене/ошибке. Предыдущий открытый проект сохраняется. Browser regression проверяет exception, видимый error banner, отсутствие unhandled rejection, снятие busy, повторное открытие и отмену. Это проверка реакции UI на transport failure, не проверка работы самого Windows picker.

### Rust super выше корня crate

Для src/child.rs с `use super::super::Thing` resolver игнорировал неуспешный pop контекста и мог выбрать корневой Thing. Сначала отдельный regression test воспроизвёл ложное подтверждённое ребро и упал.

Теперь контекст с выходом выше корня отвергается; импорт остаётся resolution issue без придуманной связи. Rust resolver profile повышен с rust-resolver-1 до rust-resolver-2; rust-parser-1 сохранён, AST cache не инвалидируется из-за изменения resolution. Исторические результаты не пересчитываются; несовместимые профили сравниваются с ограничениями.

## Матрица сценариев

| Сценарий | Проверка | Результат |
| --- | --- | --- |
| JS/TS aliases, exports/imports, workspace ambiguity | Rust fixtures | Пройдено |
| Rust/Python mixed graph, conditional/structural relations, path dependency | Rust fixtures | Пройдено в поддерживаемой модели |
| Независимые AST versions, reuse, неизвестный будущий cache format | Rust/storage tests | Пройдено |
| SQLite reopen/migration/write rollback, сохранение последнего snapshot | Rust/storage tests | Пройдено |
| Commit blobs, dirty worktree, scope, links/LFS, path limits, cancellation boundary | Git fixtures | Пройдено |
| Git status staged+unstaged, HEAD/index read-only | Git fixtures + explicit Git CLI oracle | Пройдено |
| Graph filters, directories, focus, dense/empty/large lists | Node + browser | Пройдено |
| Diagnostics evidence, graph jump, legacy snapshots | Rust + browser | Пройдено |
| Impact chain/cycles/depth/work budgets, proof pages, query expiration | Rust + browser | Пройдено |
| Git comparison → historical graph → return | Browser with exported engine data | Пройдено |
| Error/cancel folder picker и повторный запрос | Browser transport regression | Пройдено |
| Cold native startup с существующей историей | Computer Use/read-only observation | Пройдено на установленной 0.9.0 |
| Native picker cancellation, graph, save research → close/reopen | Computer Use + readonly MCP/SQLite audit | Пройдено на установленной 0.9.0 |
| Повторный native scan → Impact/Git на финальной 0.9.0 | Computer Use | Полный сквозной прогон не выполнялся; scan проверен на предыдущей QA-сборке, Impact/Git — Rust и browser |
| Native restore/maximize при текущем Windows scale 125% | Computer Use + layout observation | Пройдено; остальные системные DPI не менялись |
| Silent NSIS upgrade и запуск установленной копии | Installer /S, exit code, версия/изменение binary hash, process path | Пройдено, включая сборку с исправлением карточки |
| GUI wizard установщика и MSI upgrade | Native installer | Не выполнялось |

## История ограничений Computer Use

Использован computer-use (@oai/sky). Состояние окна и accessibility tree читаются, screenshot имеет обычные размеры, но coordinate input отклоняется:

```text
point (103, 689) is outside window bounds { originX: 0, originY: 0, width: 14, height: 14 }
```

Повторный list_windows/get_window/activate_window и свежий snapshot дали ту же ошибку. Дальнейший ввод прекращён согласно recovery instructions. Это не доказанная ошибка DEPENDENCY; автоматическое прохождение native-сценариев не подменяется DOM-моками.

Дополнительно обнаружено, что запуск через app launcher выбрал установленную 0.7.0. Для проверки версии был явно запущен workspace release executable; его путь, ProductVersion и UI v0.8.0 сверены. Пользовательская база не удалялась и не очищалась. Прямой файловый/SQL обход для имитации пользовательских действий не применялся.

## Воспроизведение автоматических проверок

Основные команды — в [development](development.md). Для explicit engine export:

```powershell
$env:DEPENDENCY_DUMP_DIR = "$env:TEMP\dependency-090-check"
$env:DEPENDENCY_MIXED_FIXTURE = Join-Path (Get-Location) 'src-tauri/tests/fixtures/native_mixed'
$env:DEPENDENCY_PROFILE_ROOTS = (Get-Location).Path + ';' + $env:DEPENDENCY_MIXED_FIXTURE
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --ignored --nocapture
node tests/ui-smoke.mjs "$env:DEPENDENCY_DUMP_DIR\git-ui.json" 'C:\Program Files\Google\Chrome\Application\chrome.exe'
node tests/ui-smoke.mjs "$env:DEPENDENCY_DUMP_DIR\impact-ui.json" 'C:\Program Files\Google\Chrome\Application\chrome.exe'
```

В read-only прогоне самого DEPENDENCY working analysis переиспользовал 76 из 81 parses; сравнение заняло около 259 ms. Для mixed subproject неизменённые 8 parses переиспользованы полностью. Это измерения конкретного прогона, не SLA. Исследуемые скрипты и lifecycle hooks не запускались.

## Что остаётся до полного native sign-off

Сохранение и восстановление исследования через настоящий WebView, picker cancellation и сохранность базы при обновлении подтверждены на установленной 0.9.0. Основной объём 0.9 завершён. За пределами выполненной проверки остаются GUI wizard/MSI upgrade (MSI собран, обновление выполнено NSIS /S), другие системные DPI, полный повторный native scan → Impact/Git и быстрые переключения проектов во время jobs. Это ограничения проверенного покрытия, а не обнаруженные неисправности. Перед широким распространением Open Beta эти сценарии следует включить в release checklist; наличие автоматических тестов их не заменяет.
