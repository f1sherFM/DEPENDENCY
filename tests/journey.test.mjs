import {test} from 'node:test';
import assert from 'node:assert/strict';
import {isCurrentImpact,impactFindings,graphForImpactRoot,findingName} from '../src/researchJourney.ts';
const summary={snapshotId:'old-snapshot',impactVersion:'impact-1',roots:['src/a.py','src/b.py'],effectiveEdgeKinds:['python_import','conditional_import'],maxDepth:1,status:'partial',partialReasons:['depth-limit'],knownAffectedCount:4,knownDirectCount:4,knownTransitiveCount:0,knownTestCandidateCount:1,coverage:{state:'known-gaps',reasons:['Dynamic module loading not resolved']}};

test('findings cannot use a previous snapshot, root set, relation set or depth',()=>{
 assert.ok(isCurrentImpact(summary,'old-snapshot',['src/b.py','src/a.py'],['conditional_import','python_import'],'1'));
 for(const args of [['new-snapshot',summary.roots,summary.effectiveEdgeKinds,'1'],['old-snapshot',['src/a.py'],summary.effectiveEdgeKinds,'1'],['old-snapshot',summary.roots,['python_import'],'1'],['old-snapshot',summary.roots,summary.effectiveEdgeKinds,'']])assert.equal(isCurrentImpact(summary,...args),false);
 assert.equal(isCurrentImpact(null,'old-snapshot',[],[],''),false);
});
test('saved findings retain lower bounds, coverage, filters and only the displayed proof segment',()=>{
 const note=impactFindings(summary,{search:'tests',testsOnly:true},{offset:200,totalEdgeCount:500,reachesRoot:false,edges:[{source:'tests/test_a.py',target:'src/a.py',line:17,kind:'python_import'}]});
 for(const text of ['old-snapshot','depth-limit','не менее 4','Dynamic module loading','201–201 из 500','показан частично','tests/test_a.py:17 → src/a.py','только кандидаты тестов: да'])assert.ok(note.includes(text),text);
 const huge=impactFindings({...summary,roots:['😀'.repeat(12000)]},{search:'',testsOnly:false},null);
 assert.ok(huge.length<16000);assert.ok(huge.includes('сокращена'));assert.equal(huge.isWellFormed(),true);assert.equal(findingName('😀'.repeat(120)).isWellFormed(),true);
});
test('impact finding restores an ordinary incoming graph on a captured file, never an invented target',()=>{
 const analysis={nodes:[{id:'src/a.py',kind:'file'},{id:'pkg:x',kind:'package'}]};
 assert.equal(graphForImpactRoot(analysis,'missing.py'),null);assert.equal(graphForImpactRoot(analysis,'pkg:x'),null);
 const graph=graphForImpactRoot(analysis,'src/a.py');assert.equal(graph.selectedNodeId,'src/a.py');assert.equal(graph.direction,'incoming');assert.equal(graph.showPackages,false);
});
