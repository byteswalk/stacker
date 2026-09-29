import fs from "node:fs";
import path from "node:path";
import ts from "typescript";

const ROOT = path.resolve(import.meta.dirname, "..");
const SOURCE_ROOT = path.join(ROOT, "src");
const RUST_SOURCE_ROOT = path.join(ROOT, "src-tauri", "src");
const CHINESE_TEXT = /[\u3400-\u9fff]/;
const ALLOWED_LITERAL_TEXT = new Set([
  "中文 / English",
  "界面语言已切换为简体中文",
]);

function readTextMap(filePath, variableName) {
  const sourceText = fs.readFileSync(filePath, "utf8");
  const sourceFile = ts.createSourceFile(filePath, sourceText, ts.ScriptTarget.Latest, true);
  const result = {};

  function collectObject(initializer) {
    for (const property of initializer.properties) {
      if (!ts.isPropertyAssignment(property)) continue;
      const key = ts.isStringLiteralLike(property.name)
        ? property.name.text
        : ts.isIdentifier(property.name)
          ? property.name.text
          : undefined;
      const value = ts.isStringLiteralLike(property.initializer)
        ? property.initializer.text
        : undefined;
      if (key !== undefined && value !== undefined) result[key] = value;
    }
  }

  function visit(node) {
    if (
      ts.isVariableDeclaration(node)
      && ts.isIdentifier(node.name)
      && node.name.text === variableName
      && node.initializer
      && ts.isObjectLiteralExpression(node.initializer)
    ) {
      collectObject(node.initializer);
    }
    if (
      ts.isCallExpression(node)
      && ts.isPropertyAccessExpression(node.expression)
      && ts.isIdentifier(node.expression.expression)
      && node.expression.expression.text === "Object"
      && node.expression.name.text === "assign"
      && ts.isIdentifier(node.arguments[0])
      && node.arguments[0].text === variableName
      && node.arguments[1]
      && ts.isObjectLiteralExpression(node.arguments[1])
    ) {
      collectObject(node.arguments[1]);
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return result;
}

function walk(directory) {
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) return walk(fullPath);
    return [fullPath];
  });
}

const generated = readTextMap(path.join(SOURCE_ROOT, "en.generated.ts"), "GENERATED_EN");
const curated = readTextMap(path.join(SOURCE_ROOT, "i18n.ts"), "CURATED_EN");
const translations = { ...generated, ...curated };
const phrases = Object.keys(translations)
  .filter((value) => CHINESE_TEXT.test(value))
  .sort((left, right) => right.length - left.length);

function isCovered(value) {
  const original = value.trim();
  if (!CHINESE_TEXT.test(original) || ALLOWED_LITERAL_TEXT.has(original)) return true;
  if (translations[original]) return true;
  const text = original.replace(/\{[^{}]*\}/g, "");

  let remaining = text;
  for (const phrase of phrases) {
    if (remaining.includes(phrase)) remaining = remaining.split(phrase).join("");
  }
  return !CHINESE_TEXT.test(remaining);
}

function collectVisibleText(filePath) {
  const sourceText = fs.readFileSync(filePath, "utf8");
  if (sourceText.includes("\uFFFD")) {
    throw new Error(`Invalid replacement character found in ${path.relative(ROOT, filePath)}`);
  }

  const sourceFile = ts.createSourceFile(
    filePath,
    sourceText,
    ts.ScriptTarget.Latest,
    true,
    filePath.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const findings = [];

  function record(node, text) {
    const normalized = text.replace(/\s+/g, " ").trim();
    if (!normalized || isCovered(normalized)) return;
    const position = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
    findings.push({
      file: path.relative(ROOT, filePath),
      line: position.line + 1,
      text: normalized,
    });
  }

  function visit(node) {
    if (ts.isStringLiteralLike(node) || ts.isJsxText(node)) record(node, node.text);
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return findings;
}

const sourceFiles = walk(SOURCE_ROOT).filter((filePath) => {
  if (!/\.tsx?$/.test(filePath)) return false;
  if (/\.(test|spec)\.tsx?$/.test(filePath)) return false;
  return !filePath.endsWith("en.generated.ts") && !filePath.endsWith("i18n.ts");
});

const findings = sourceFiles.flatMap(collectVisibleText);
const rustFindings = walk(RUST_SOURCE_ROOT)
  .filter((filePath) => filePath.endsWith(".rs"))
  .flatMap((filePath) => {
    const wholeFile = fs.readFileSync(filePath, "utf8");
    if (wholeFile.includes("\uFFFD")) {
      throw new Error(`Invalid replacement character found in ${path.relative(ROOT, filePath)}`);
    }
    // A test's fixtures are not interface text, so the trailing test module is left out of
    // the count, the same way .test.ts files are.
    const testsAt = wholeFile.search(/^#\[cfg\(test\)\]\r?\nmod tests \{/m);
    const sourceText = testsAt === -1 ? wholeFile : wholeFile.slice(0, testsAt);
    return sourceText.split(/\r?\n/).flatMap((line, index) => {
      if (line.trimStart().startsWith("//")) return [];
      const values = [];
      const pattern = /"((?:\\.|[^"\\])*)"/g;
      let match;
      while ((match = pattern.exec(line)) !== null) {
        const text = match[1].replace(/\\n/g, " ").replace(/\\"/g, "\"").trim();
        if (CHINESE_TEXT.test(text) && !isCovered(text)) {
          values.push({ file: path.relative(ROOT, filePath), line: index + 1, text });
        }
      }
      return values;
    });
  });

findings.push(...rustFindings);
if (findings.length > 0) {
  console.error(`Internationalization coverage failed: ${findings.length} untranslated UI strings.`);
  for (const finding of findings) {
    console.error(`${finding.file}:${finding.line}  ${finding.text}`);
  }
  process.exit(1);
}

console.log(`Internationalization coverage passed (${Object.keys(translations).length} translations).`);
