import { test } from 'node:test';
import assert from 'node:assert/strict';
import { summarizeCoverage, groupCoverage, coverageDescription } from '../src/coverage.ts';

function fixture() {
  const resolutionIssues = [
    ['Rust macros are not expanded; generated dependencies may be absent',545],
    ['Rust use requires unresolved item/reexport/external-crate context; no target is guessed',71],
    ['Conditional Rust use: cfg profile is unknown',19],
    ['Conditional Rust module: active cfg profile is unknown',2],
  ].flatMap(([reason,count])=>Array.from({length:count},(_,index)=>({source:`src/${index%3}.rs`,specifier:'example',line:index+1,status:'unsupported',reason,attemptedCandidates:[],hint:''})));
  return {summary:{resolutionIssues,parseErrorCount:0},rulesRun:{coverageIssueCount:637},diagnostics:[]};
}
test('637 raw occurrences remain four limitations even with diagnostic rules disabled',()=>{
  const analysis=fixture(); const before=JSON.stringify(analysis);
  const summary=summarizeCoverage(analysis);
  assert.equal(summary.unsupportedCount,637); assert.equal(summary.limitations.length,4);
  assert.deepEqual(summary.limitations.map(item=>item.count),[545,71,19,2]);
  assert.equal(summary.limitations[0].fileCount,3);
  assert.equal(summary.unresolvedCount+summary.ambiguousCount+summary.parseErrorFiles+summary.otherCoverageCount,0);
  assert.equal(JSON.stringify(analysis),before);
});
test('real resolution errors, parse failures, capture omissions and unexplained records stay separate',()=>{
  const analysis=fixture();
  for(const status of ['unresolved','ambiguous','future-resolution-state','external'])analysis.summary.resolutionIssues.push({status,source:'main.ts',reason:'reason'});
  analysis.summary.parseErrorCount=2; analysis.basis={omittedCount:3};
  analysis.rulesRun.coverageIssueCount=647; // 637 + 3 resolution + 2 parse + 3 capture + 2 unexplained
  const summary=summarizeCoverage(analysis);
  assert.equal(summary.unresolvedCount,1); assert.equal(summary.ambiguousCount,1);
  assert.equal(summary.parseErrorFiles,2); assert.equal(summary.captureOmissions,3);
  assert.equal(summary.otherResolutionCount,1); assert.equal(summary.otherCoverageCount,2);
  const legacy={summary:{resolutionIssues:[],parseErrorCount:0},rulesRun:{coverageIssueCount:7}};
  assert.equal(summarizeCoverage(legacy).otherCoverageCount,7);
});
test('grouped presentation preserves every saved diagnostic and unknown explanations',()=>{
  const diagnostics=fixture().summary.resolutionIssues.map((issue,index)=>({id:String(index),category:'coverage',severity:'warning',ruleId:'resolution-unsupported',explanation:issue.reason,evidence:{type:'resolution',issue}}));
  diagnostics.push({id:'architecture',category:'architecture'});
  const groups=groupCoverage(diagnostics);
  assert.equal(groups.length,4); assert.equal(groups.flat().length,637);
  assert.equal(new Set(groups.flat().map(item=>item.id)).size,637);
  assert.equal(coverageDescription('constructor').title,'constructor');
  assert.equal(coverageDescription('future limitation').title,'future limitation');
});
