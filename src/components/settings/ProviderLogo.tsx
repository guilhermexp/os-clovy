import { IconAnthropic } from "central-icons/IconAnthropic";
import { IconClaudeai } from "central-icons/IconClaudeai";
import { IconDeepseek } from "central-icons/IconDeepseek";
import { IconGemini } from "central-icons/IconGemini";
import { IconGrok } from "central-icons/IconGrok";
import { IconMetaAi } from "central-icons/IconMetaAi";
import { IconMistral } from "central-icons/IconMistral";
import { IconNvidia } from "central-icons/IconNvidia";
import { IconOllama } from "central-icons/IconOllama";
import { IconOpenai } from "central-icons/IconOpenai";
import { IconPerplexity } from "central-icons/IconPerplexity";
import { BrandMark, Monogram, initials } from "../brand/BrandMark";
import { ClovyMark } from "../brand/ClovyLogo";

type ProviderLogoProps = {
  provider: string;
  id: string;
  name?: string;
  size?: number;
};

export function ProviderLogo({ provider, id, name = "", size = 18 }: ProviderLogoProps) {
  const kind = classifyProvider(provider, id, name);
  switch (kind) {
    case "clovy":
      return (
        <ClovyMark
          width={Math.round(size * 0.78)}
          height={Math.round(size * 0.78)}
          label="Clovy"
          variant="mono"
        />
      );
    case "openai":
      return <IconOpenai size={size} aria-label="OpenAI" />;
    case "anthropic":
      return <IconClaudeai size={size} aria-label="Claude" />;
    case "google":
      return <IconGemini size={size} aria-label="Gemini" />;
    case "meta":
      return <IconMetaAi size={size} aria-label="Meta" />;
    case "mistral":
      return <IconMistral size={size} aria-label="Mistral" />;
    case "deepseek":
      return <IconDeepseek size={size} aria-label="DeepSeek" />;
    case "perplexity":
      return <IconPerplexity size={size} aria-label="Perplexity" />;
    case "ollama":
      return <IconOllama size={size} aria-label="Ollama" />;
    case "xai":
      return <IconGrok size={size} aria-label="Grok" />;
    case "nvidia":
      return <IconNvidia size={size} aria-label="NVIDIA" />;
    case "elevenlabs":
      return <ElevenLabsMark size={size} />;
    case "openrouter":
      return <BrandMark brand="openrouter" size={size} />;
    default:
      return <Monogram label={initials(name || id || provider)} size={size} />;
  }
}

// Anthropic gets two aliases — IconAnthropic and IconClaudeai. We prefer the
// product mark (Claude) because that's the visible product name.
export { IconAnthropic };

type ProviderKind =
  | "clovy"
  | "openai"
  | "anthropic"
  | "google"
  | "meta"
  | "mistral"
  | "deepseek"
  | "perplexity"
  | "ollama"
  | "xai"
  | "nvidia"
  | "elevenlabs"
  | "openrouter"
  | "unknown";

function classifyProvider(provider: string, id: string, name: string): ProviderKind {
  // Auto is a Clovy-managed route even when its catalog record inherits a
  // provider value from the currently available routing backends.
  if (id.toLowerCase() === "open-software/auto") return "clovy";

  // Model family takes precedence over hosting platform (e.g. Groq-hosted
  // Whisper still reads as OpenAI). Hosting providers only win when nothing
  // more specific matched.
  const normalizedProvider = provider.toLowerCase();
  const haystack = `${normalizedProvider} ${id} ${name}`.toLowerCase();

  if (
    normalizedProvider === "openai" ||
    haystack.includes("gpt-") ||
    haystack.includes("whisper-") ||
    haystack.includes("o1-") ||
    haystack.includes("o3-") ||
    haystack.includes("o4-")
  ) {
    return "openai";
  }
  if (
    normalizedProvider === "anthropic" ||
    haystack.includes("claude") ||
    haystack.includes("anthropic")
  ) {
    return "anthropic";
  }
  if (haystack.includes("gemini") || haystack.includes("google")) {
    return "google";
  }
  if (haystack.includes("llama") || haystack.includes("meta-")) return "meta";
  if (haystack.includes("mistral") || haystack.includes("mixtral")) {
    return "mistral";
  }
  if (haystack.includes("deepseek")) return "deepseek";
  if (haystack.includes("perplexity") || haystack.includes("pplx")) {
    return "perplexity";
  }
  if (haystack.includes("ollama")) return "ollama";
  if (normalizedProvider === "xai" || haystack.includes("grok") || haystack.includes("xai/")) {
    return "xai";
  }
  if (haystack.includes("nvidia") || haystack.includes("parakeet")) {
    return "nvidia";
  }
  if (haystack.includes("eleven")) return "elevenlabs";

  if (haystack.includes("openai")) return "openai";
  // The routing service itself, only once no model family matched.
  if (normalizedProvider === "openrouter") return "openrouter";

  return "unknown";
}

// Visual weight calibration. Custom SVGs fill their viewBox unevenly, so at
// equal pixel size their marks read larger or smaller than central-icons.
// These factors approximate the central-icons baseline (~85% viewBox fill).
const ELEVENLABS_SCALE = 1.05;

function ElevenLabsMark({ size }: { size: number }) {
  const s = Math.round(size * ELEVENLABS_SCALE);
  return (
    <svg
      width={s}
      height={s}
      viewBox="0 0 876 876"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="ElevenLabs"
    >
      <path d="M468 292H528V584H468V292Z" fill="currentColor" />
      <path d="M348 292H408V584H348V292Z" fill="currentColor" />
    </svg>
  );
}
