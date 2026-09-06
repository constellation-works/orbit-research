/* No imported HTML, dynamic code, remote requests, plugins, or telemetry. */
'use strict';
const data = window.RESEARCH_DATA;
const nodes = new Map(data.records.map(n => [n.key, n]));
const $ = id => document.getElementById(id);
const text = (tag, value, cls) => {
  const e = document.createElement(tag);
  e.textContent = value;
  if (cls) e.className = cls;
  return e;
};
const pretty = value => typeof value === 'string' ? value : JSON.stringify(value, null, 2);
const badge = value => text('span', value, 'badge ' + value);
const title = n => [n.record.presentation.title, n.record.payload.title, n.record.payload.name, n.record.aliases[0], n.record.id].find(v => typeof v === 'string' && v);
const searchable = new Map(data.records.map(n => [n.key, JSON.stringify(n.record).toLocaleLowerCase()]));
let selected = null;

function section(parent, label) {
  const block = text('section', '', 'section');
  block.append(text('h3', label)); parent.append(block); return block;
}
function raw(parent, label, value) {
  const d = document.createElement('details');
  d.append(text('summary', label), text('pre', pretty(value)));
  parent.append(d);
}
function facts(parent, values) {
  const dl = text('dl', '', 'facts');
  for (const [label, value] of Object.entries(values)) {
    dl.append(text('dt', label), text('dd', value == null || value === '' ? 'Not recorded' : pretty(value)));
  }
  parent.append(dl);
}
function link(parent, n) {
  const button = text('button', title(n));
  button.type = 'button';
  button.addEventListener('click', () => select(n.key, true));
  parent.append(button);
}
function closure(n) {
  const seen = new Set(), queue = [n.key];
  while (queue.length) {
    const k = queue.pop();
    if (seen.has(k)) continue;
    seen.add(k);
    const item = nodes.get(k);
    queue.push(...item.assessments, ...item.edges.map(e => e.target).filter(Boolean));
  }
  return [...seen].map(k => nodes.get(k));
}
function openQuestion(n) {
  return n.record.kind === 'claim' && !n.assessments.some(k => nodes.get(k).confirmation === 'eligible');
}
function filtered() {
  const query = $('search').value.toLocaleLowerCase().trim();
  return data.records.filter(n => {
    if ($('owner').value && n.pin[0] !== $('owner').value) return false;
    if ($('kind').value && n.record.kind !== $('kind').value) return false;
    if (query && !searchable.get(n.key).includes(query)) return false;
    const view = $('view').value;
    return !view || (view === 'open' && openQuestion(n)) ||
      (view === 'controls' && n.axes.controls === 'failed') ||
      (view === 'pending' && n.reconciliation === 'pending') ||
      (view === 'current' && n.confirmation === 'eligible') ||
      (view === 'historical' && n.record.kind === 'assessment' && n.history === 'historical');
  });
}
function renderResults() {
  const visible = filtered();
  $('count').textContent = `${visible.length} of ${data.records.length} records`;
  $('results').replaceChildren();
  for (const n of visible) {
    const b = text('button', '', 'result'); b.type = 'button';
    b.setAttribute('aria-pressed', String(n.key === selected));
    b.append(text('span', `${n.pin[0]} / ${n.record.kind}`, 'meta'), text('strong', title(n)),
      badge(n.record.kind === 'assessment' ? n.axes.verdict : n.reconciliation));
    b.addEventListener('click', () => select(n.key, true));
    $('results').append(b);
  }
  if (!visible.length) $('results').append(text('p', 'No matching records. Clear a filter or try an alias.', 'empty'));
  return visible;
}
function select(k, focus = false) {
  if (!nodes.has(k)) {
    selected = null; renderResults();
    $('detail').replaceChildren(text('h2', 'Snapshot not found'), text('p', 'This exact record is absent from this export. Rebuild with its owner manifest; no newer revision was substituted.'));
    return;
  }
  selected = k;
  if (location.hash !== '#' + k) history.replaceState(null, '', '#' + k);
  renderResults(); renderDetail(nodes.get(k));
  if (focus) {
    $('detail').focus({preventScroll: true});
    if (matchMedia('(max-width:650px)').matches) $('detail').scrollIntoView({behavior:'auto',block:'start'});
  }
}
function renderDetail(n) {
  const host = $('detail'); host.replaceChildren();
  const r = n.record, p = r.payload;
  const claimEdge = n.edges.find(e => p.claim && e.reference.id === p.claim.id && e.reference.revision_id === p.claim.revision_id && e.reference.source_revision === p.claim.source_revision);
  const claimPayload = claimEdge?.target ? nodes.get(claimEdge.target).record.payload : p;
  host.append(text('div', `${n.pin[0]} / ${r.kind} / ${n.history}`, 'tagline'), text('h2', title(n)));
  const axes = text('div', '', 'axes');
  for (const [axis, value] of Object.entries(n.axes)) {
    const a = text('div', '', 'axis'); a.append(text('small', axis.toUpperCase()), badge(value)); axes.append(a);
  }
  host.append(axes);
  const callout = text('div', '', 'callout');
  callout.append(badge(n.reconciliation), text('p', n.confirmation === 'eligible' ?
    'Owner-authored current confirmatory assessment; exact closure passes the framework checks.' :
    n.history === 'historical' ? 'Historical owner report. Its recorded verdict is preserved; it is not a current confirmatory assessment.' :
    'No current confirmation from this record. Follow the exact evidence and outstanding obligations below.'));
  if (n.confirmation === 'conflicting') callout.append(text('p', 'Current assessments disagree. An owner decision is required.'));
  host.append(callout);
  if (claimPayload.statement || p.question || p.semantic?.question) host.append(text('p', claimPayload.statement || p.question || p.semantic.question, 'statement'));
  const scope = section(host, 'Claim, scope & assumptions');
  facts(scope, {Domain:claimPayload.domain, Scope:r.scope, Aliases:r.aliases, Assumptions:claimPayload.assumptions || p.semantic?.assumptions,
    Limitations:r.limitations.length ? r.limitations : 'None recorded; not proof of unrestricted applicability'});
  if (r.kind === 'assessment') {
    const block = section(host, 'Evidence assessment');
    facts(block, {Verdict:p.verdict, Inference:p.inference, Basis:p.basis, 'Evidence direction':p.evidence_summary,
      'Recorded legacy verdict':p.legacy_verdict, Rationale:p.rationale});
  }
  if (r.kind === 'experiment') {
    const block = section(host, 'Execution & controls');
    facts(block, {Execution:p.execution_status, Controls:p.controls, 'Control results':p.control_results,
      Deviations:p.deviations, 'Started at':p.started_at, 'Finished at':p.finished_at,
      'Code commit':p.code, 'Input pins':p.inputs, 'Holdout digest':p.holdout_digest,
      Environment:p.environment, 'Recorded invocation (never executed)':p.invocation});
  }
  if (r.kind === 'protocol') {
    const block = section(host, 'Frozen protocol');
    facts(block, {Freeze:p.freeze, 'Frozen at':p.frozen_at, 'Semantic digest':p.semantic_digest});
    raw(block, 'Full normative terms, design and code/data pins', p.semantic);
  }
  if (r.kind === 'artifact') {
    const block = section(host, 'Artifact availability');
    facts(block, {Role:p.role, Availability:p.availability, 'Snapshot digest':p.snapshot_digest,
      'Locator (not automatically fetched)':p.locator, 'Media type':p.media_type});
  }
  const related = section(host, 'Assessments of this exact claim snapshot');
  if (!n.assessments.length) related.append(text('p', 'No assessments of this exact source/revision pin in this export.', 'muted'));
  for (const k of n.assessments) {
    const a = nodes.get(k), row = text('div', '', 'relation');
    link(row, a); row.append(text('p', `${a.history} · ${a.axes.verdict} · controls ${a.axes.controls} · ${a.reconciliation}`));
    row.append(text('p', a.record.payload.rationale)); related.append(row);
  }
  const obligations = section(host, 'Next obligations');
  const reasons = [...n.reasons, ...n.edges.filter(e => e.status === 'pending').map(e => `${e.reference.id}: ${e.reason}`)];
  const next = p.next_obligations || r.presentation.next_obligations;
  if (next) reasons.push(pretty(next));
  if (n.axes.controls === 'failed') reasons.push('Resolve failed controls before primary inference; completed execution does not settle the claim.');
  if (n.history === 'historical') reasons.push('Owner must author any new assessment and establish chronology; this history cannot be relabelled as preregistered evidence.');
  if (!reasons.length) reasons.push('No unresolved index checks. Further scientific obligations remain the owner’s decision.');
  const ul = text('ul', '', 'obligations'); for (const reason of new Set(reasons)) ul.append(text('li', reason)); obligations.append(ul);
  const edges = section(host, 'Exact dependency pins');
  for (const e of n.edges) {
    const row = text('div', '', 'relation'); row.append(badge(e.status), document.createTextNode(' '));
    if (e.target) link(row, nodes.get(e.target)); else row.append(text('span', e.reference.id));
    row.append(text('p', `${e.reference.repository} · ${e.reference.revision_id} · source ${e.reference.source_revision || 'not recorded'}`, 'pin'));
    if (e.reason) row.append(text('p', e.reason)); edges.append(row);
  }
  if (!n.edges.length) edges.append(text('p', 'No typed dependency pins recorded. Prose references remain in the source record.', 'muted'));
  const chain = closure(n);
  const trace = section(host, 'Reproducibility trace');
  trace.append(text('p', `${chain.length} exact snapshots, including assessments, protocols, runs and data when recorded. Missing pins remain above.`, 'muted'));
  const tree = text('div', '', 'tree');
  for (const item of chain) {
    const row = text('div', '', 'relation'); row.append(text('span', `${item.record.kind} · ${item.pin[0]} · `));
    link(row, item); row.append(text('p', `${item.history} · ${item.reconciliation} · ${item.axes.execution} · ${item.axes.verdict}`, 'muted'));
    const code = item.record.payload.code || item.record.payload.semantic?.code;
    if (code) row.append(text('p', `Code: ${code.repository} @ ${code.git_revision}`, 'pin'));
    tree.append(row);
  }
  trace.append(tree);
  const media = (data.media || []).filter(m => chain.some(item => item.key === m.record));
  const mediaSection = section(host, 'Figures & simulations');
  if (!media.length) mediaSection.append(text('p', 'No explicitly mapped figures or simulations. Imported links are retained as source text; no remote content is fetched.', 'muted'));
  const grid = text('div', '', 'media-grid');
  for (const m of media) {
    const card = text('div', '', 'media'); card.append(text('h4', m.label), badge(m.role), text('p', m.state));
    if (m.image) { const img = document.createElement('img'); img.src = m.image; img.alt = `${m.label} (${m.role})`; img.loading = 'lazy'; card.append(img); }
    card.append(text('p', m.reason));
    if (m.url) { const a = text('a', m.role === 'simulation' ? 'Open simulation ↗' : 'Open artifact ↗'); a.href = m.url; a.target = '_blank'; a.rel = 'noopener noreferrer'; card.append(a); }
    grid.append(card);
  }
  mediaSection.append(grid);
  const source = section(host, 'Source provenance & revision history');
  facts(source, {Identity:r.id, 'Semantic revision':r.revision_id, 'Exact source commit':r.provenance.git_revision,
    'Owner path':r.provenance.path, Selector:r.provenance.selector, 'Source SHA-256':r.provenance.sha256,
    'Revision heads':n.heads, 'Supersedes':r.authorship?.supersedes, 'Orbit execution links':r.orbit_links,
    'Raw owner activity':r.legacy?.frontmatter?.status || r.legacy?.status});
  raw(source, 'Original owner record (unchanged)', r);
  raw(source, 'Full source content and legacy qualifications', r.legacy);
  if (n.variants.length) raw(source, 'Conflicting exact record variants', n.variants);
}

for (const owner of data.owners) { const o = text('option', owner); o.value = owner; $('owner').append(o); }
for (const kind of [...new Set(data.records.map(n => n.record.kind))].sort()) { const o = text('option', kind); o.value = kind; $('kind').append(o); }
for (const [value, label] of [[data.owners.length,'owners'], [data.records.filter(openQuestion).length,'open questions'],
  [data.records.filter(n => n.axes.controls === 'failed').length,'failed controls'],
  [data.records.filter(n => n.reconciliation === 'pending').length,'pending records']]) {
  const s = text('div', '', 'stat'); s.append(text('strong', value), document.createTextNode(label)); $('stats').append(s);
}
for (const id of ['search','owner','kind','view']) $(id).addEventListener('input', () => {
  const visible = renderResults();
  if (visible.length && !visible.some(n => n.key === selected)) select(visible[0].key);
  if (!visible.length) { selected = null; $('detail').replaceChildren(text('h2', 'No matching records'), text('p', 'Clear a filter or try a different search.')); }
});
$('snapshot').textContent = `${data.notice} Content digest ${data.content_digest}`;
$('unresolved').querySelector('summary').textContent = `Unresolved manifest targets (${data.unresolved.length})`;
for (const u of data.unresolved) $('unresolved').querySelector('div').append(text('pre', pretty(u)));
window.addEventListener('hashchange', () => select(location.hash.slice(1)));
if (location.hash) select(location.hash.slice(1));
else if (data.records.length) select((data.records.find(openQuestion) || data.records[0]).key);
else $('detail').append(text('h2','No owner records'),text('p','Only manifest targets are present. Add their owner records and rebuild.'));
