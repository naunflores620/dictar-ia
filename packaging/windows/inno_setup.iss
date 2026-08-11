; Instalador de Windows para dictar_ia.
;
; `flutter build windows` produce un .exe que NO es distribuible: depende de las
; DLL y de data\ que tiene al lado. Esto empaqueta esa carpeta en un único .exe
; de instalación, que es lo que se descarga el usuario.
;
;   iscc packaging\windows\inno_setup.iss
;
; Nota: Flutter ya copia msvcp140.dll y vcruntime140*.dll en la carpeta Release,
; así que NO hace falta exigir el Visual C++ Redistributable.

#define NombreApp "dictar_ia"
#define Version   "0.1.0"
#define Editor    "dictar_ia"
#define Ejecutable "dictar_ia.exe"

[Setup]
AppId={{7C1F4A2E-9B3D-4E5A-8F62-1D0C5B7A4E93}
AppName={#NombreApp}
AppVersion={#Version}
AppPublisher={#Editor}
DefaultDirName={autopf}\{#NombreApp}
DefaultGroupName={#NombreApp}
OutputDir=..\..\dist
OutputBaseFilename=dictar_ia-{#Version}-windows-setup
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Instalación por usuario: sin diálogo de UAC. Para una aplicación personal es
; lo cómodo; solo haría falta `admin` si instalara un servicio o un driver.
PrivilegesRequired=lowest
UninstallDisplayIcon={app}\{#Ejecutable}
WizardStyle=modern
DisableProgramGroupPage=yes

[Languages]
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; \
  GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\app\build\windows\x64\runner\Release\*"; DestDir: "{app}"; \
  Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\config\providers.toml"; DestDir: "{userappdata}\dictar_ia"; \
  Flags: onlyifdoesntexist

[Icons]
Name: "{group}\{#NombreApp}";       Filename: "{app}\{#Ejecutable}"
Name: "{autodesktop}\{#NombreApp}"; Filename: "{app}\{#Ejecutable}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#Ejecutable}"; Description: "{cm:LaunchProgram,{#NombreApp}}"; \
  Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Los modelos de Whisper pesan ~850 MB y se descargan aparte. Se borran al
; desinstalar, pero NUNCA los datos del usuario: sus grabaciones y apuntes
; sobreviven a una reinstalación.
Type: filesandordirs; Name: "{localappdata}\dictar_ia\models"
