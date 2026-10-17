param([Parameter(Mandatory=$true)][string]$OutputPath)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {
    $voice = $synth.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -eq 'pt-BR' } | Select-Object -First 1
    if ($voice) { $synth.SelectVoice($voice.VoiceInfo.Name) }
    $format = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(16000, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
    $synth.SetOutputToWaveFile($OutputPath, $format)
    $synth.Speak('Este é um teste do Dicta. O valor é trinta reais, não vinte. A reunião será na segunda-feira às dez horas.')
} finally { $synth.Dispose() }
Write-Output "Synthetic test fixture created: $OutputPath"
