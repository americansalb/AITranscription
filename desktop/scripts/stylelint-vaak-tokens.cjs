/**
 * stylelint-vaak-tokens — pattern-(c) typed enforcement for the Vaak CSS layer.
 *
 * Six rules enforce that primitive values (color, spacing, font-size, radius,
 * z-index) come from the design tokens in `src/styles/tokens.css` instead of
 * being hardcoded. Plus one meta-rule that requires every stylelint-disable
 * comment to carry a justification comment on the next line.
 *
 * Spec: .vaak/design-notes/typed-css-spec-2026-05-13.md (a2c1315) +
 *       corrigendum 584568b (Findings 1-4 addressed).
 *
 * Wave 0 ship: all five primitive rules in WARN mode; the disable-justification
 * rule in ERROR mode. Wave 2/3/4 flip primitives to ERROR per file-pattern
 * overrides as V2 / V1 adopt the tokens.
 */

const stylelint = require("stylelint");
const fs = require("fs");
const path = require("path");
const { colord } = require("colord");

// ---------------------------------------------------------------------------
// Token loader — parses src/styles/tokens.css at lint-time to derive allow-sets
// ---------------------------------------------------------------------------

const TOKENS_PATH = path.resolve(__dirname, "../src/styles/tokens.css");

class TokensMissingError extends Error {
  constructor(message) {
    super(message);
    this.code = "VAAK_TOKENS_MISSING";
  }
}

class TokensParseError extends Error {
  constructor(message) {
    super(message);
    this.code = "VAAK_TOKENS_PARSE_ERROR";
  }
}

let tokenCache = null;

function loadTokens() {
  if (tokenCache) return tokenCache;

  if (!fs.existsSync(TOKENS_PATH)) {
    throw new TokensMissingError(
      `Token source of truth not found: ${TOKENS_PATH}. ` +
      `Wave 1 of typed-css-spec requires this file to exist before the plugin runs.`
    );
  }

  let raw;
  try {
    raw = fs.readFileSync(TOKENS_PATH, "utf8");
  } catch (e) {
    throw new TokensParseError(`Failed to read tokens.css: ${e.message}`);
  }

  // Naive but sufficient: extract `--<name>: <value>;` pairs. Stylelint's own
  // parser could be reused but adds complexity for a one-file read.
  const entries = {};
  const re = /--([a-z0-9-]+)\s*:\s*([^;]+);/gi;
  let m;
  while ((m = re.exec(raw)) !== null) {
    entries[m[1]] = m[2].trim();
  }

  if (Object.keys(entries).length === 0) {
    throw new TokensParseError(
      "tokens.css parsed but yielded zero custom properties. " +
      "Plugin cannot proceed against an empty allow-set."
    );
  }

  // Build typed allow-sets per primitive
  const colors = new Set();
  const spacings = new Set();
  const fontSizes = new Set();
  const radii = new Set();
  const zIndices = new Set();

  for (const [name, value] of Object.entries(entries)) {
    if (/^space-\d+/.test(name)) spacings.add(value);
    else if (/^text-/.test(name)) fontSizes.add(value);
    else if (/^radius-/.test(name)) radii.add(value);
    else if (/^z-/.test(name)) zIndices.add(value);
    // Colors are heuristic — if the value parses as a color, include it.
    else if (colord(value).isValid()) colors.add(colord(value).toHex().toLowerCase());
  }

  tokenCache = { colors, spacings, fontSizes, radii, zIndices };
  return tokenCache;
}

// ---------------------------------------------------------------------------
// Rule implementations
// ---------------------------------------------------------------------------

const ruleMessages = stylelint.utils.ruleMessages;

function makeColorRule() {
  const ruleName = "vaak-tokens/no-raw-color";
  const messages = ruleMessages(ruleName, {
    rejected: (value) =>
      `Raw color "${value}" not in design tokens. Use a CSS custom property from tokens.css or add a justified exemption.`,
  });
  return {
    rule: stylelint.createPlugin(ruleName, (enabled) => (root, result) => {
      if (!enabled) return;
      const tokens = loadTokens();
      root.walkDecls((decl) => {
        // Skip declarations inside :root (token definitions themselves)
        if (decl.parent.selector === ":root") return;
        // Match any color-y string
        const re = /(#[0-9a-fA-F]{3,8}\b|rgba?\([^)]+\)|hsla?\([^)]+\))/g;
        const seen = new Set();
        let m;
        while ((m = re.exec(decl.value)) !== null) {
          const raw = m[1];
          if (seen.has(raw)) continue;
          seen.add(raw);
          const c = colord(raw);
          if (!c.isValid()) continue;
          const canonical = c.toHex().toLowerCase();
          if (!tokens.colors.has(canonical)) {
            stylelint.utils.report({
              result,
              ruleName,
              message: messages.rejected(raw),
              node: decl,
              word: raw,
            });
          }
        }
      });
    }),
    name: ruleName,
    messages,
  };
}

function makePxRule(kind, allowedKey, propRegex) {
  const ruleName = `vaak-tokens/no-raw-${kind}`;
  const messages = ruleMessages(ruleName, {
    rejected: (value) =>
      `Raw ${kind} "${value}" not in design tokens. Use a CSS custom property from tokens.css or add a justified exemption.`,
  });
  return {
    rule: stylelint.createPlugin(ruleName, (enabled) => (root, result) => {
      if (!enabled) return;
      const tokens = loadTokens();
      root.walkDecls((decl) => {
        if (decl.parent.selector === ":root") return;
        if (propRegex && !propRegex.test(decl.prop)) return;
        const re = /\b(\d+(?:\.\d+)?(?:px|rem|em))\b/g;
        const seen = new Set();
        let m;
        while ((m = re.exec(decl.value)) !== null) {
          const raw = m[1];
          if (seen.has(raw)) continue;
          seen.add(raw);
          if (!tokens[allowedKey].has(raw)) {
            // Special case: 0 (unitless) is always allowed
            if (raw === "0" || raw === "0px") continue;
            stylelint.utils.report({
              result,
              ruleName,
              message: messages.rejected(raw),
              node: decl,
              word: raw,
            });
          }
        }
      });
    }),
    name: ruleName,
    messages,
  };
}

function makeZIndexRule() {
  const ruleName = "vaak-tokens/no-raw-z-index";
  const messages = ruleMessages(ruleName, {
    rejected: (value) =>
      `Raw z-index "${value}" not in design tokens. Use --z-base / --z-overlay / --z-popover / --z-modal / --z-toast / --z-cursor from tokens.css.`,
  });
  return {
    rule: stylelint.createPlugin(ruleName, (enabled) => (root, result) => {
      if (!enabled) return;
      const tokens = loadTokens();
      root.walkDecls("z-index", (decl) => {
        const raw = decl.value.trim();
        // var() refs pass through
        if (/^var\(--/.test(raw)) return;
        if (!tokens.zIndices.has(raw)) {
          stylelint.utils.report({
            result,
            ruleName,
            message: messages.rejected(raw),
            node: decl,
            word: raw,
          });
        }
      });
    }),
    name: ruleName,
    messages,
  };
}

function makeDisableJustificationRule() {
  const ruleName = "vaak-tokens/no-disable-without-justification";
  const messages = ruleMessages(ruleName, {
    rejected: () =>
      `stylelint-disable comment requires a justification comment on the next line ` +
      `(e.g., /* Justification: brand gradient, single-use, see ticket #1234 */). ` +
      `Per typed-css-spec corrigendum 584568b Finding 2.`,
  });
  return {
    rule: stylelint.createPlugin(ruleName, (enabled) => (root, result) => {
      if (!enabled) return;
      root.walkComments((comment) => {
        const text = comment.text.trim();
        if (!/^stylelint-disable(-next-line)?\b/.test(text)) return;
        // Look at the comment immediately after the disable's affected line
        const next = comment.next();
        if (next && next.type === "comment" && /justification/i.test(next.text)) return;
        // Allow inline justification on the same comment
        if (/justification/i.test(text)) return;
        stylelint.utils.report({
          result,
          ruleName,
          message: messages.rejected(),
          node: comment,
        });
      });
    }),
    name: ruleName,
    messages,
  };
}

// ---------------------------------------------------------------------------
// Plugin export
// ---------------------------------------------------------------------------

const rules = [
  makeColorRule(),
  makePxRule("spacing", "spacings", /^(margin|padding|gap|inset|top|right|bottom|left|width|height)/),
  makePxRule("font-size", "fontSizes", /^font-size$/),
  makePxRule("radius", "radii", /^border-radius$/),
  makeZIndexRule(),
  makeDisableJustificationRule(),
];

module.exports = rules.map((r) => r.rule);
