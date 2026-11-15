export interface UpdateStatus {
  sequence: number;
  phase: string;
  version: string;
}

export function updateMessage(status: UpdateStatus): string {
  switch (status.phase) {
    case "downloading":
      return `Baixando Dicta ${status.version} em segundo plano…`;
    case "waiting":
      return `Dicta ${status.version} pronto. A atualização será instalada ao terminar as tarefas em andamento.`;
    case "installing":
      return `Instalando Dicta ${status.version}. O aplicativo será reaberto automaticamente…`;
    case "error":
      return "Não foi possível atualizar o Dicta. Tentaremos novamente em um minuto.";
    default:
      return "";
  }
}
