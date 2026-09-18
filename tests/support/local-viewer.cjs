// Execute the served viewer page against a scripted exchange with the local
// query service. The page must ask for exactly the bounded requests the Rust
// core answers, in order, and must render what comes back without deriving any
// relationship of its own.
const assert = require('node:assert/strict');
const vm = require('node:vm');
const input = JSON.parse(require('node:fs').readFileSync(0, 'utf8'));
const exchanges = input.exchanges.slice();

function element(tag) {
  const node = {
    tagName: tag,
    children: [],
    attributes: {},
    className: '',
    textContent: '',
    classList: {
      classes: new Set(),
      toggle(name) {
        if (this.classes.has(name)) {
          this.classes.delete(name);
          return false;
        }
        this.classes.add(name);
        return true;
      },
    },
    append(...kids) {
      for (const kid of kids) this.children.push(kid);
    },
    replaceChildren(...kids) {
      this.children = [...kids];
    },
    hasChildNodes() {
      return this.children.length > 0;
    },
    setAttribute(name, value) {
      this.attributes[name] = String(value);
    },
    getAttribute(name) {
      return this.attributes[name];
    },
    querySelector(selector) {
      return descendants(this).find(node => node.className &&
        node.className.split(' ').includes(selector.slice(1))) || null;
    },
  };
  return node;
}

function descendants(node) {
  const found = [];
  for (const child of node.children || []) {
    if (typeof child !== 'object') continue;
    found.push(child, ...descendants(child));
  }
  return found;
}

function text(node) {
  return [node.textContent || '', ...(node.children || []).map(
    child => typeof child === 'object' ? text(child) : String(child))].join(' ');
}

const registry = new Map();
const document = {
  getElementById(id) {
    if (!registry.has(id)) registry.set(id, element(id));
    return registry.get(id);
  },
  createElement: element,
  createElementNS(_namespace, tag) {
    return element(tag);
  },
};

async function fetch(path, init) {
  const expected = exchanges.shift();
  assert.ok(expected, `unexpected extra request to ${path}`);
  assert.equal(path, expected.path);
  if (expected.request === undefined) {
    assert.equal(init.method, undefined, `${path} must be a plain read`);
  } else {
    assert.equal(init.method, 'POST');
    assert.deepEqual(JSON.parse(init.body), expected.request,
      `${path} must send exactly the bounded request the core defines`);
  }
  return {ok: true, json: async () => expected.response};
}

const context = vm.createContext({document, fetch});
vm.runInContext(input.html.split('<script>')[1].split('</script>')[0], context);
const settle = () => new Promise(resolve => setTimeout(resolve, 0));
const $ = id => document.getElementById(id);
// The page reads the selected policy from its own control; a real select
// reports its first option before anyone changes it.
$('policy').value = 'include-possible';
const find = (id, predicate) => descendants($(id)).find(predicate);
const button = (id, label) => find(id, node => node.tagName === 'button' && text(node).trim() === label);

(async () => {
  await settle();
  // A build target and an observation context are selected before anything is
  // explored, and exploring is refused until one is chosen.
  assert.ok(text($('snapshot')).includes(input.program_snapshot_id));
  assert.equal($('target').children.length, input.build_target_count);
  assert.equal($('contexts').children.length, input.observation_context_count);
  assert.equal($('search-run').disabled, false);
  assert.equal($('run-path').disabled, true, 'a path needs a start and an end');

  $('search').value = input.searches[0];
  await $('search-run').onclick();
  await settle();
  assert.ok($('matches').children.length > 0, 'search renders its matched callables');
  await button('matches', 'Start here').onclick();
  assert.equal($('focus-panel').hidden, false);

  await $('run-callees').onclick();
  await settle();
  assert.ok($('result').querySelector('.diagram'), 'a focused neighborhood is drawn');
  assert.ok(descendants($('result')).some(node => text(node).includes(input.expected_callee)),
    'the rendered neighborhood names the callee the core reported');
  assert.ok(text($('result')).includes('Open-world projection'));

  const evidence = button('result', 'Evidence');
  assert.equal(evidence.getAttribute('aria-expanded'), 'false');
  await evidence.onclick();
  await settle();
  assert.equal(evidence.getAttribute('aria-expanded'), 'true');
  const expanded = descendants($('result')).find(node => node.className === 'evidence');
  assert.ok(expanded.hasChildNodes(), 'an explanation handle expands into evidence details');
  assert.ok(text(expanded).includes(input.expected_evidence),
    'expanded evidence shows what the core explained');

  $('mark-start').onclick();
  $('search').value = input.searches[1];
  await $('search-run').onclick();
  await settle();
  await button('matches', 'Start here').onclick();
  $('mark-end').onclick();
  assert.equal($('run-path').disabled, false);
  await $('run-path').onclick();
  await settle();
  assert.ok($('result').querySelector('.diagram'), 'a focused path is drawn');
  assert.ok(text($('result')).includes('relationship(s)'));

  assert.equal(exchanges.length, 0, 'every scripted request was made');
})().catch(error => {
  console.error(error);
  process.exit(1);
});
