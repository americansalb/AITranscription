import { LANGUAGES } from "../lib/languages";

interface LanguageSelectorProps {
  value: string;
  onChange: (code: string) => void;
  label?: string;
  disabled?: boolean;
}

export function LanguageSelector({
  value,
  onChange,
  label = "Language",
  disabled,
}: LanguageSelectorProps) {
  return (
    <div className="language-selector">
      <label htmlFor="lang-select">{label}</label>
      <select
        id="lang-select"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={disabled}
      >
        {LANGUAGES.map((l) => (
          <option key={l.code} value={l.code}>
            {l.name}
          </option>
        ))}
      </select>
    </div>
  );
}
