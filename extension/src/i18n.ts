// Popup copy in English and Brazilian Portuguese. The extension runs inside
// the browser and cannot read the Clovy app's interface-language preference,
// so it follows the browser's UI language (the platform convention for
// extensions) and falls back to English for anything else.

export type ExtensionLocale = "en" | "pt-BR";

const en = {
  checking: "Checking connection...",
  tryAgain: "Try again",
  shareHeading: "Share this tab",
  shareIntro: "Only the tab you choose becomes available to the current Clovy task.",
  shareButton: "Share this tab",
  stopSharing: "Stop sharing",
  cancelShare: "Cancel share",
  copyShareCode: "Copy share code",
  notConnectedTitle: "Not connected",
  notConnectedDetail: "Clovy is not connected to this browser yet.",
  connectingTitle: "Connecting",
  connectingDetail: "Reaching the Clovy app...",
  handshakingDetail: "Confirming versions with the Clovy app...",
  pairedTitle: "Connected to Clovy",
  pairedDetail: "Clovy can open its own tabs in this browser when you ask it to.",
  unreachableTitle: "Clovy is not running",
  unreachableDetail: "Open the Clovy app, then try again.",
  updateRequiredTitle: "Update required",
  updateApp: "This extension is newer than the Clovy app. Update Clovy, then try again.",
  updateExtension:
    "The Clovy app is newer than this extension. Update the Clovy extension, then try again.",
  updateBoth:
    "This extension and the Clovy app speak different versions. Update both, then try again.",
  tabShared: "This tab is shared with the current Clovy task.",
  tabOwned: "This tab already belongs to the current Clovy task.",
  shareCode: "Share code: {code}. Paste it into your Clovy chat.",
  preparing: "Preparing...",
  shareCodeCopied: "Share code copied. Paste it into your Clovy chat.",
  openTabFirst: "Open a browser tab before sharing.",
  tabCannotBeShared: "This tab cannot be shared.",
  tabCouldNotBeShared: "This tab could not be shared.",
  noActiveTab: "No active browser tab was found.",
  connectBeforeSharing: "Connect the Clovy app before sharing a tab.",
  sharingFailed: "The tab could not be shared.",
};

export type ExtensionMessageKey = keyof typeof en;

const ptBR: Record<ExtensionMessageKey, string> = {
  checking: "Verificando a conexão...",
  tryAgain: "Tentar de novo",
  shareHeading: "Compartilhar esta aba",
  shareIntro: "Só a aba que você escolher fica disponível para a tarefa atual do Clovy.",
  shareButton: "Compartilhar esta aba",
  stopSharing: "Parar de compartilhar",
  cancelShare: "Cancelar compartilhamento",
  copyShareCode: "Copiar código de compartilhamento",
  notConnectedTitle: "Não conectado",
  notConnectedDetail: "O Clovy ainda não está conectado a este navegador.",
  connectingTitle: "Conectando",
  connectingDetail: "Procurando o app Clovy...",
  handshakingDetail: "Confirmando versões com o app Clovy...",
  pairedTitle: "Conectado ao Clovy",
  pairedDetail: "O Clovy pode abrir as próprias abas neste navegador quando você pedir.",
  unreachableTitle: "O Clovy não está aberto",
  unreachableDetail: "Abra o app Clovy e tente de novo.",
  updateRequiredTitle: "Atualização necessária",
  updateApp: "Esta extensão é mais nova que o app Clovy. Atualize o Clovy e tente de novo.",
  updateExtension:
    "O app Clovy é mais novo que esta extensão. Atualize a extensão do Clovy e tente de novo.",
  updateBoth:
    "Esta extensão e o app Clovy usam versões diferentes. Atualize os dois e tente de novo.",
  tabShared: "Esta aba está compartilhada com a tarefa atual do Clovy.",
  tabOwned: "Esta aba já pertence à tarefa atual do Clovy.",
  shareCode: "Código de compartilhamento: {code}. Cole na sua conversa do Clovy.",
  preparing: "Preparando...",
  shareCodeCopied: "Código copiado. Cole na sua conversa do Clovy.",
  openTabFirst: "Abra uma aba do navegador antes de compartilhar.",
  tabCannotBeShared: "Esta aba não pode ser compartilhada.",
  tabCouldNotBeShared: "Não foi possível compartilhar esta aba.",
  noActiveTab: "Nenhuma aba ativa do navegador foi encontrada.",
  connectBeforeSharing: "Conecte o app Clovy antes de compartilhar uma aba.",
  sharingFailed: "Não foi possível compartilhar a aba.",
};

const catalogs: Record<ExtensionLocale, Record<ExtensionMessageKey, string>> = {
  en,
  "pt-BR": ptBR,
};

export function resolveExtensionLocale(tag: string | undefined | null): ExtensionLocale {
  const normalized = (tag ?? "").trim().replace(/_/g, "-").toLowerCase();
  return normalized === "pt" || normalized.startsWith("pt-") ? "pt-BR" : "en";
}

export function browserLocale(): ExtensionLocale {
  const chromeI18n = (globalThis as { chrome?: { i18n?: { getUILanguage?: () => string } } }).chrome
    ?.i18n;
  const tag =
    chromeI18n?.getUILanguage?.() ??
    (typeof navigator === "undefined" ? undefined : navigator.language);
  return resolveExtensionLocale(tag);
}

export function tx(
  key: ExtensionMessageKey,
  params: Record<string, string> = {},
  locale: ExtensionLocale = browserLocale(),
): string {
  const template = catalogs[locale][key] ?? en[key];
  return template.replace(/\{(\w+)\}/g, (match, name: string) => params[name] ?? match);
}
