import {test} from 'node:test';
import assert from 'node:assert/strict';
import {architectureMap} from '../src/architecture.ts';
const node=path=>({id:path,path,label:path.split('/').at(-1),kind:'file'});
const edge=(source,target,kind='rust_import',id=source+target+kind)=>({id,source,target,kind,line:3});
test('overview aggregates every file and relation without changing snapshot or losing evidence',()=>{
 const nodes=['src/a.rs','src/b.rs','tests/a.rs','build.rs'].map(node);
 const edges=[edge(nodes[0].id,nodes[1].id),edge(nodes[2].id,nodes[0].id),edge(nodes[2].id,nodes[1].id,'conditional_import')];
 const before=JSON.stringify({nodes,edges});const result=architectureMap(nodes,edges);
 assert.equal(result.nodes.length,3);assert.equal(result.internal,1);assert.equal(result.edges.length,1);assert.equal(result.edges[0].count,2);assert.equal(result.edges[0].evidence.length,2);assert.equal(result.edges[0].kinds.length,2);assert.equal(JSON.stringify({nodes,edges}),before);
});
test('drill-down separates files, directories and outside scope; prefixes cannot match sibling paths',()=>{
 const nodes=['src/a.py','src/deep/b.py','src2/c.py'].map(node);const edges=[edge(nodes[0].id,nodes[1].id),edge(nodes[0].id,nodes[2].id)];
 const nested=architectureMap(nodes,edges,'src');assert.equal(nested.total,2);assert.equal(nested.boundary,1);assert.equal(nested.edges.length,1);
 assert.equal(nested.boundaries[0].direction,'outgoing');assert.equal(nested.boundaries[0].folder,'src2');assert.deepEqual(nested.boundaries[0].evidence,[edges[1]]);
 const files=architectureMap(nodes,edges,'src',true);assert.deepEqual(files.nodes.map(n=>n.id),['src/a.py']);assert.equal(files.boundary,2);
});

test('boundary evidence distinguishes incoming from outgoing and excludes package declarations',()=>{
 const nodes=['src/a.py','tests/a.py','tests/b.py','main.py'].map(node);
 const edges=[edge('tests/a.py','src/a.py'),edge('tests/b.py','src/a.py'),edge('src/a.py','tests/a.py'),edge('main.py','src/a.py'),edge('src/a.py','pkg:external','package')];
 const result=architectureMap(nodes,edges,'src');assert.equal(result.boundary,4);assert.equal(result.boundaries.length,3);
 const incoming=result.boundaries.find(b=>b.direction==='incoming'&&b.folder==='tests');assert.equal(incoming.count,2);assert.deepEqual(incoming.evidence,edges.slice(0,2));
});
test('large maps account for hidden relations and include isolated groups; order is input independent',()=>{
 const nodes=Array.from({length:80},(_,i)=>node(`dir-${i}/a.py`));const edges=nodes.slice(1).map(n=>edge(nodes[0].id,n.id));
 const result=architectureMap(nodes,edges);assert.equal(result.nodes.length,12);assert.equal(result.hidden,68);assert.equal(result.edges.reduce((sum,e)=>sum+e.count,0)+result.hiddenRelations,79);
 assert.deepEqual(architectureMap([...nodes].reverse(),[...edges].reverse()).nodes,result.nodes);
 assert.equal(architectureMap([],[]).total,0);
});
