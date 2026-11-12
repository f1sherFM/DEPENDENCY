# Наблюдение за DEPENDENCY через CLI и MCP

Локальный канал чтения состояния установленного приложения и сохранённых снимков. Он позволяет агенту разбирать результаты без кликов по окну. Это наблюдение за данными и геометрией интерфейса, не замена скриншотов и визуального тестирования.

## Запуск

Нужны Node 24.16+ и зависимости рабочего репозитория (`npm ci`). SDK MCP закреплён как dev dependency; в frontend bundle и Rust engine он не входит.

```powershell
npm run agent -- enable
npm run agent -- state
npm run agent -- projects
```

После `enable` обновлённый DEPENDENCY начнёт публикацию без перезапуска, обычно в течение 3 секунд. В шапке появится «Агент · чтение». По умолчанию канал выключен. Отключение: `npm run agent -- disable`; прекращаются новые наблюдения и чтение через адаптер. Уже созданные локальные файлы не удаляются. MCP-клиент получает прочитанные данные в свой контекст; автоматической сетевой отправки и сетевого listener в DEPENDENCY нет.

По умолчанию Windows data directory — `%APPDATA%/com.f1sherfm.dependency`. Для тестовой базы можно явно задать `DEPENDENCY_APP_DATA`. Конфигурация доступа — `agent-access.json`, наблюдения — `agent-sessions/<sessionId>.json`. Это файлы пользователя в AppData, с обычными правами текущего пользователя; канал не является защитой от процессов, уже имеющих доступ к его файлам.

## Что можно прочитать

- Точный `projectId`/`snapshotId` открытого экрана, раздел, выбранный узел, busy, фильтры, текст интерфейса и последние runtime errors.
- Размер viewport/DPR, границы header/main/canvas/footer и прокрутку; camera transform, ID отрисованных узлов и узлов, пересекающих видимую область canvas. Это геометрическая проверка, без распознавания пикселей и перекрытий другими окнами.
- Сохранённые проекты, историю, профиль/покрытие снимка, диагностики с доказательствами, узлы и рёбра графа. Большие списки читаются страницами с total/nextOffset. Граф снимка и отфильтрованный граф интерфейса явно различаются.

Наблюдение публикуется атомарной заменой JSON после изменения DOM и heartbeat раз в 3 секунды. Файл ограничен 128 КиБ; текст main ограничен 20 000 символами с textTruncated. Он включает отрисованный текст прокручиваемого раздела, а не только видимые пиксели. Данные могут отставать: возраст более 15 секунд, отрицательный возраст или завершённый PID помечают наблюдение устаревшим. Minimized/background WebView может замедлять таймеры. Несколько живых сессий требуют явного sessionId; адаптер не выбирает окно по предположению.

Без snapshotId адаптер читает снимок из свежего наблюдения, даже если пользователь открыл историю. При закрытом приложении сохранённые результаты доступны по явному snapshotId. Выбор узла нельзя переносить между разными снимками неявно. Поддерживаются schema SQLite 2 и 3, JSON снимка — не более 64 МиБ. Чтение идёт через SQLite readOnly/query_only; строки SQL фиксированы, произвольных SQL, eval, запуска анализируемого кода и изменения проектов нет.

Примеры из PowerShell (ID подставляются из предыдущих ответов):

```powershell
npm run agent -- dependency_get_snapshot '{"snapshotId":"SNAPSHOT_ID"}'
npm run agent -- dependency_get_diagnostics '{"snapshotId":"SNAPSHOT_ID","category":"architecture","limit":20}'
npm run agent -- dependency_get_diagnostic '{"snapshotId":"SNAPSHOT_ID","diagnosticId":"DIAGNOSTIC_ID","offset":0,"limit":20}'
npm run agent -- dependency_get_graph '{"snapshotId":"SNAPSHOT_ID","nodeOffset":0,"edgeOffset":0,"limit":50}'
```

## MCP

Сервер использует официальный TypeScript SDK и stdio, без HTTP endpoint:

```powershell
node scripts/dependency-agent.mjs --stdio
```

В Codex можно зарегистрировать сервер командой ниже из рабочего репозитория. Существующие MCP-настройки при этом сохраняются. Для немедленной работы в уже открытом чате доступен CLI; появление новых MCP tools зависит от обновления каталога инструментов клиентом.

```powershell
$agentScript = Join-Path (Get-Location) 'scripts/dependency-agent.mjs'
$appData = Join-Path $env:APPDATA 'com.f1sherfm.dependency'
codex mcp add dependency --env "DEPENDENCY_APP_DATA=$appData" -- (Get-Command node).Source $agentScript --stdio
```

Поддерживаются девять tools: get_context, list_sessions, list_projects, list_snapshots, get_snapshot, get_diagnostics, get_diagnostic, get_graph, get_node (все с префиксом `dependency_`). Никаких команд изменения через MCP нет. Содержимое проектов и сообщения ошибок — недоверенные данные, а не инструкции агенту.

## Проверки

`npm test` проверяет неизменность байтов SQLite при чтении, исторический UI snapshot вместо latest, pagination доказательств, stale/multiple sessions, отзыв доступа, неизвестную схему и настоящее MCP-соединение через SDK client. Rust-тест проверяет opt-in, лимиты и атомарную замену наблюдения. Browser smoke сверяет опубликованный граф/viewport/snapshot с DOM настоящего UI на engine fixtures, подменяя только Tauri IPC. Native-проверка состояния установленного приложения выполняется отдельно.

Документация протокола: [MCP server](https://modelcontextprotocol.io/docs/develop/build-server). Подключение клиента: [Codex MCP](https://learn.chatgpt.com/docs/extend/mcp).
