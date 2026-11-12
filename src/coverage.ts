import type { AnalysisResult, Diagnostic } from "./types";

const descriptions: Record<string, { title: string; description: string }> = {
  "Rust macros are not expanded; generated dependencies may be absent": {
    title: "Макросы Rust не раскрываются",
    description: "Анализатор видит вызовы макросов, но не вычисляет сгенерированный ими код. Созданные макросами зависимости могут отсутствовать в графе.",
  },
  "Rust use requires unresolved item/reexport/external-crate context; no target is guessed": {
    title: "Не хватает контекста символов Rust",
    description: "Для подтверждения цели use нужен дополнительный контекст символов, реэкспортов или внешнего crate. Предполагаемые связи не добавлены в граф.",
  },
  "Conditional Rust use: cfg profile is unknown": {
    title: "Неизвестна конфигурация условных импортов Rust",
    description: "Анализатор не выбирает активную ветку cfg. Связи из условных импортов могут отсутствовать в графе.",
  },
  "Conditional Rust module: active cfg profile is unknown": {
    title: "Неизвестна конфигурация условных модулей Rust",
    description: "Активный профиль cfg не определён. Состав условных модулей и их связи могут быть неполными.",
  },
};

export function coverageDescription(reason: string) {
  return (Object.hasOwn(descriptions, reason) ? descriptions[reason] : undefined) ?? { title: reason, description: "Сохранённое сообщение о покрытии анализа. Подробности и места приведены ниже." };
}

export function groupCoverage(diagnostics: readonly Diagnostic[]) {
  const groups = new Map<string, Diagnostic[]>();
  for (const item of diagnostics) {
    if (item.category !== "coverage") continue;
    const key = JSON.stringify([item.ruleId, item.severity, item.explanation]);
    const group = groups.get(key);
    if (group) group.push(item); else groups.set(key, [item]);
  }
  return [...groups.values()];
}

// Coverage facts are independent of enabled diagnostic rules and UI filters.
// Keep the raw count for compatibility; do not reinterpret unknown records as success.
export function summarizeCoverage(analysis: AnalysisResult) {
  const issues = analysis.summary.resolutionIssues ?? [];
  const unsupported = issues.filter(issue => issue.status === "unsupported");
  const groups = new Map<string, typeof unsupported>();
  for (const issue of unsupported) {
    const group = groups.get(issue.reason);
    if (group) group.push(issue); else groups.set(issue.reason, [issue]);
  }
  const parseErrorFiles = analysis.summary.parseErrorCount;
  const captureOmissions = analysis.basis?.omittedCount ?? 0;
  const knownRecords = issues.filter(issue => issue.status !== "external").length + parseErrorFiles + captureOmissions;
  return {
    unsupportedCount: unsupported.length,
    limitations: [...groups].map(([reason, records]) => ({ reason, ...coverageDescription(reason), count: records.length, fileCount: new Set(records.map(issue => issue.source)).size })).sort((a, b) => b.count - a.count || a.title.localeCompare(b.title)),
    unresolvedCount: issues.filter(issue => issue.status === "unresolved").length,
    ambiguousCount: issues.filter(issue => issue.status === "ambiguous").length,
    otherResolutionCount: issues.filter(issue => !["unsupported", "unresolved", "ambiguous", "external"].includes(issue.status)).length,
    parseErrorFiles, captureOmissions,
    otherCoverageCount: Math.max(0, (analysis.rulesRun?.coverageIssueCount ?? 0) - knownRecords),
    recordedCount: analysis.rulesRun?.coverageIssueCount ?? null,
  };
}
