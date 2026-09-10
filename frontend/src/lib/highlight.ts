/**
 * A tiny, dependency-free tokenizer for the code samples on this site.
 * It covers the handful of languages the docs actually use and is not
 * meant to be a general-purpose highlighter: swapping in Shiki later
 * only means changing this file and `CodeBlock`.
 */

export type TokenType =
  | "comment"
  | "string"
  | "number"
  | "keyword"
  | "command"
  | "flag"
  | "punct"
  | "plain";

export type Token = { type: TokenType; value: string };

export type Language =
  | "bash"
  | "python"
  | "js"
  | "ts"
  | "go"
  | "rust"
  | "resp"
  | "json"
  | "text";

const KEYWORDS: Record<string, string[]> = {
  python: ["import", "from", "as", "def", "class", "return", "if", "elif", "else", "for", "while", "in", "with", "try", "except", "print", "None", "True", "False", "async", "await", "lambda"],
  js: ["import", "from", "export", "default", "const", "let", "var", "function", "return", "if", "else", "for", "while", "new", "await", "async", "class", "try", "catch", "null", "undefined", "true", "false", "console", "type", "interface"],
  ts: ["import", "from", "export", "default", "const", "let", "var", "function", "return", "if", "else", "for", "while", "new", "await", "async", "class", "try", "catch", "null", "undefined", "true", "false", "console", "type", "interface", "string", "number", "boolean"],
  go: ["package", "import", "func", "return", "if", "else", "for", "range", "var", "const", "type", "struct", "defer", "nil", "true", "false", "string", "int", "error"],
  rust: ["pub", "struct", "enum", "impl", "fn", "let", "mut", "match", "use", "mod", "self", "Some", "None", "Option", "Vec", "usize", "f32", "return", "if", "else", "for", "in", "where"],
  bash: ["docker", "cargo", "npm", "pnpm", "yarn", "pip", "go", "curl", "export", "sudo", "sh", "brew", "redis-cli", "nc", "klyro"],
};

/** Commands that should read as commands in a RESP transcript. */
const RESP_COMMAND = /^(?:MEM\.[A-Z]+|[A-Z][A-Z0-9]{1,15})\b/;

const OPTION_WORDS = new Set([
  "MODE", "DIM", "METRIC", "WEIGHTS", "HALFLIFE", "TOPK", "FILTER", "META",
  "IMPORTANCE", "TTL", "TEXT", "VEC", "FVEC", "ID", "NX", "XX", "EX", "PX",
  "WITHSCORES", "WITHMETA", "WITHVEC", "NOTEXT", "FUSION", "LINEAR", "RRF",
  "SEARCH", "VECTOR", "HYBRID", "COSINE", "L2", "IP", "COUNT", "MATCH",
  "EQ", "NE", "GT", "GTE", "LT", "LTE", "IN", "CONTAINS", "KEEPTTL",
]);

type Rule = [TokenType, RegExp];

function rulesFor(lang: Language): Rule[] {
  const string: Rule = [
    "string",
    /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`/y,
  ];
  const number: Rule = ["number", /\b\d+(?:\.\d+)?\b/y];
  const punct: Rule = ["punct", /[{}()[\];,.:=<>+\-*/|&!?]/y];
  const hash: Rule = ["comment", /#[^\n]*/y];
  const slash: Rule = ["comment", /\/\/[^\n]*|\/\*[\s\S]*?\*\//y];

  switch (lang) {
    case "bash":
      return [hash, string, number, ["keyword", wordsRe(KEYWORDS.bash)], ["flag", /--?[a-zA-Z][\w-]*/y], punct];
    case "python":
      return [hash, string, number, ["keyword", wordsRe(KEYWORDS.python)], punct];
    case "js":
    case "ts":
      return [slash, string, number, ["keyword", wordsRe(KEYWORDS[lang])], punct];
    case "go":
      return [slash, string, number, ["keyword", wordsRe(KEYWORDS.go)], punct];
    case "rust":
      return [slash, string, number, ["keyword", wordsRe(KEYWORDS.rust)], punct];
    case "json":
      return [string, number, ["keyword", /\b(?:true|false|null)\b/y], punct];
    default:
      return [hash, string, number, punct];
  }
}

function wordsRe(words: string[]) {
  return new RegExp(`\\b(?:${words.join("|")})\\b`, "y");
}

/** Transcripts of a `redis-cli`/`nc` session get their own pass. */
function tokenizeResp(code: string): Token[] {
  const out: Token[] = [];
  for (const line of code.split("\n")) {
    if (out.length) out.push({ type: "plain", value: "\n" });
    if (line.startsWith("#") || line.startsWith("//")) {
      out.push({ type: "comment", value: line });
      continue;
    }
    // Reply lines from the server keep their RESP type marker.
    if (/^[+\-:$*]/.test(line)) {
      out.push({ type: "string", value: line });
      continue;
    }
    const words = line.split(/(\s+)/);
    let first = true;
    for (const word of words) {
      if (!word.trim()) {
        out.push({ type: "plain", value: word });
        continue;
      }
      if (first && RESP_COMMAND.test(word)) {
        out.push({ type: "command", value: word });
        first = false;
        continue;
      }
      first = false;
      if (OPTION_WORDS.has(word)) out.push({ type: "keyword", value: word });
      else if (/^"?-?\d+(\.\d+)?"?$/.test(word)) out.push({ type: "number", value: word });
      else if (/^".*"$/.test(word)) out.push({ type: "string", value: word });
      else out.push({ type: "plain", value: word });
    }
  }
  return out;
}

export function tokenize(code: string, lang: Language = "text"): Token[] {
  if (lang === "resp") return tokenizeResp(code);

  const rules = rulesFor(lang);
  const tokens: Token[] = [];
  let index = 0;
  let buffer = "";

  const flush = () => {
    if (buffer) {
      tokens.push({ type: "plain", value: buffer });
      buffer = "";
    }
  };

  while (index < code.length) {
    let matched = false;
    for (const [type, regex] of rules) {
      regex.lastIndex = index;
      const match = regex.exec(code);
      if (match && match[0].length > 0) {
        flush();
        tokens.push({ type, value: match[0] });
        index += match[0].length;
        matched = true;
        break;
      }
    }
    if (!matched) {
      buffer += code[index];
      index += 1;
    }
  }
  flush();
  return tokens;
}

export const TOKEN_CLASS: Record<TokenType, string> = {
  comment: "text-ink-faint italic",
  string: "text-mint",
  number: "text-amber",
  keyword: "text-brand-bright",
  command: "text-cyan font-medium",
  flag: "text-cyan",
  punct: "text-ink-muted",
  plain: "text-ink",
};
