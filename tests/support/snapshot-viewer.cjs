// Execute the generated standalone viewer, including expansion and escaping.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const input = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const cards = [];
const root = {appendChild(card) {cards.push(card)}};
const document = {
  querySelector(selector) {return selector === '#relationships' ? root : {}},
  createElement() {
    let open = false;
    const button = {setAttribute(name, value) {this[name] = value}};
    const panel = {innerHTML: '', hasChildNodes() {return this.innerHTML.length > 0}};
    return {classList: {toggle() {return open = !open}},
      querySelector(selector) {return selector === '.summary' ? button : panel}};
  },
};
const context = vm.createContext({document});
vm.runInContext(input.html.split('<script>')[1].split('</script>')[0], context);
const actual = JSON.parse(vm.runInContext('JSON.stringify(DATA.call_sites.map(explain))', context));
assert.equal(cards.length, input.explanations.length);
for (let i = 0; i < cards.length; i++) {
  for (const field of ['target_claims', 'correspondence_claims', 'evidence_records', 'derivations']) {
    const ordered = records => [...records].sort((a, b) => (a.id || a.output_claim_id).localeCompare(b.id || b.output_claim_id));
    assert.deepEqual(ordered(actual[i][field]), ordered(input.explanations[i][field]), field);
  }
  const panel = cards[i].querySelector('.explanation');
  const button = cards[i].querySelector('.summary');
  assert.equal(panel.innerHTML, '', 'details remain lazy until expanded');
  button.onclick();
  assert.equal(button['aria-expanded'], 'true');
  for (const evidence of actual[i].evidence_records) {
    const escaped = vm.runInContext(`escapeHtml(${JSON.stringify(evidence.id)})`, context);
    assert.ok(panel.innerHTML.includes(escaped), 'expanded evidence includes every supporting ID');
    const provenance = vm.runInContext(`escapeHtml(${JSON.stringify(evidence.source_location.artifact)})`, context);
    assert.ok(panel.innerHTML.includes(`${provenance}:${evidence.source_location.line}`));
  }
  button.onclick();
  assert.equal(button['aria-expanded'], 'false');
}
const payload = JSON.parse(vm.runInContext('JSON.stringify(DATA)', context));
assert.equal(new Set(payload.evidence_records.map(record => record.id)).size, payload.evidence_records.length);
assert.ok(payload.call_sites.every(item => !Object.hasOwn(item, 'explanation')));
