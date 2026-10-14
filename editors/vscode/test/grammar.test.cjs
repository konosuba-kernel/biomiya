const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const { test, before } = require('node:test');
const { Registry, parseRawGrammar } = require('vscode-textmate');
const { loadWASM, OnigScanner, OnigString } = require('vscode-oniguruma');

const root = path.resolve(__dirname, '..');
let grammar;
before(async () => {
    const wasm = readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
    await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
    const registry = new Registry({
        onigLib: Promise.resolve({
            createOnigScanner: patterns => new OnigScanner(patterns),
            createOnigString: text => new OnigString(text)
        }),
        loadGrammar: async scope => scope === 'source.biomiya'
            ? parseRawGrammar(readFileSync(path.join(root, 'syntaxes/biomiya.tmLanguage.json'), 'utf8'), 'biomiya.json')
            : null
    });
    grammar = await registry.loadGrammar('source.biomiya');
});

function hasScope(result, line, fragment, scope) {
    const index = line.indexOf(fragment);
    assert.ok(index >= 0, `Missing fragment ${fragment}`);
    const token = result.tokens.find(token => token.startIndex <= index && token.endIndex > index);
    assert.ok(token?.scopes.includes(scope), `${fragment}: expected ${scope}, got ${token?.scopes}`);
}

test('nested comments persist across lines and close at the correct nesting depth', () => {
    let result = grammar.tokenizeLine('/* outer /* nested');
    const line = '*/ fn hidden() {} */ fn visible() -> u32 { return 0; }';
    result = grammar.tokenizeLine(line, result.ruleStack);
    hasScope(result, line, 'hidden', 'comment.block.biomiya');
    hasScope(result, line, 'visible', 'entity.name.function.biomiya');
    hasScope(result, line, 'return', 'keyword.control.biomiya');
});

test('strings contain comment markers, accept supported escapes and mark unknown escapes', () => {
    const line = String.raw`let s = "中文 // /* \n \x21 \" \q"; // return`;
    const result = grammar.tokenizeLine(line);
    hasScope(result, line, '// /*', 'string.quoted.double.biomiya');
    for (const escape of [String.raw`\n`, String.raw`\x21`, String.raw`\"`]) {
        hasScope(result, line, escape, 'constant.character.escape.biomiya');
    }
    hasScope(result, line, String.raw`\q`, 'invalid.illegal.escape.biomiya');
    hasScope(result, line, 'return', 'comment.line.double-slash.biomiya');
});

test('unfinished strings and line comments do not consume the next line', () => {
    for (const first of ['let s = "unfinished', 'let s = "unfinished\\', '// fn hidden']) {
        const state = grammar.tokenizeLine(first).ruleStack;
        const line = 'fn next() -> u32 { return 0; }';
        hasScope(grammar.tokenizeLine(line, state), line, 'next', 'entity.name.function.biomiya');
    }
});

test('numbers, builtins, types and wrapping operators follow S-next', () => {
    const line = 'let x: usize = 0xff_u8 +% 0b10_u32 + 1_024_usize; core::ptr::<u32>(0);';
    const result = grammar.tokenizeLine(line);
    for (const number of ['0xff_u8', '0b10_u32', '1_024_usize']) {
        hasScope(result, line, number, 'constant.numeric.integer.biomiya');
        const token = result.tokens.find(t => t.startIndex === line.indexOf(number));
        assert.equal(token.endIndex - token.startIndex, number.length);
    }
    hasScope(result, line, 'usize', 'storage.type.primitive.biomiya');
    hasScope(result, line, '+%', 'keyword.operator.biomiya');
    hasScope(result, line, 'core::ptr', 'support.function.biomiya');
    const structure = 'pub struct Point { x: u32 }';
    hasScope(grammar.tokenizeLine(structure), structure, 'Point', 'entity.name.type.biomiya');
});

test('basic indentation ignores braces in strings and line comments', () => {
    const config = JSON.parse(readFileSync(path.join(root, 'language-configuration.json')));
    const increase = new RegExp(config.indentationRules.increaseIndentPattern);
    const decrease = new RegExp(config.indentationRules.decreaseIndentPattern);
    for (const line of ['fn run() -> u32 {', 'if true { // explanation', 'call(', 'let a = [']) {
        assert.ok(increase.test(line), line);
    }
    for (const line of ['// {', 'let s = "{";', 'fn run() -> u32 { return 0; }']) {
        assert.ok(!increase.test(line), line);
    }
    for (const line of ['}', '    ];', '  )']) assert.ok(decrease.test(line), line);
});
