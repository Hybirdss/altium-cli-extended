{ altium-cli-extended bridge (protocol 2).

  Runs inside Altium Designer via
    X2.EXE -RScriptingSystem:RunScript(ProjectName=C:\altium-cli-extended\bridge\HASH\AltiumCliBridge.PrjScr|ProcName=Bridge.pas>Run)
  Reads C:\altium-cli-extended\jobs\current.txt -> job id, then jobs\ID\request.ini (op=..., key=value,
  UTF-8 with BOM), appends progress to log.txt, writes response.json and finally the empty marker
  file "done".

  Rules for this file (DelphiScript):
  - Pascal brace comments do not nest: never write a closing brace inside one.
  - Try/Except does NOT catch "Undeclared identifier": a misspelt or missing API member stops the
    script in the debugger behind a modal error box. Only use members verified on AD26.
  - Real runtime errors (nil objects, Altium exceptions) are caught and reported as ok=false
    with the stage they happened in.
  - Coordinates go out as a one-member object with the key "$c" (Altium internal units,
    1e-4 mil); the CLI converts them.
  - Strings go out ASCII-only (\uXXXX escapes), so the encoding of the saved file never matters. }

Var
  JobDir : String;
  Data   : String;
  Stage  : String;
  Req    : TStringList;
  LogBuf : TStringList;
  Failed : Boolean;

{ ---------- JSON ---------- }

Function Esc(S : String) : String;
Var I, C : Integer; R, Ch : String;
Begin
  R := '';
  For I := 1 To Length(S) Do
  Begin
    Ch := Copy(S, I, 1);
    C := Ord(S[I]);
    If Ch = '\' Then R := R + '\\'
    Else If Ch = '"' Then R := R + '\"'
    Else If C = 13 Then R := R + '\r'
    Else If C = 10 Then R := R + '\n'
    Else If C = 9 Then R := R + '\t'
    Else If C < 32 Then R := R + ' '
    Else If C > 126 Then R := R + '\u' + IntToHex(C, 4)
    Else R := R + Ch;
  End;
  Result := R;
End;

Function JStr(S : String) : String;
Begin
  Result := '"' + Esc(S) + '"';
End;

Function JBool(B : Boolean) : String;
Begin
  If B Then Result := 'true' Else Result := 'false';
End;

Function JInt(N : Integer) : String;
Begin
  Result := IntToStr(N);
End;

{ A coordinate in internal units. }
Function JC(N : Integer) : String;
Begin
  Result := '{"$c":' + IntToStr(N) + '}';
End;

{ A float with a dot whatever the Windows locale. }
Function JF(X : Double) : String;
Begin
  Result := StringReplace(FormatFloat('0.####', X), ',', '.', MkSet(rfReplaceAll));
End;

Procedure Put(Key : String; JsonValue : String);
Begin
  If Data <> '' Then Data := Data + ',';
  Data := Data + JStr(Key) + ':' + JsonValue;
End;

Function Member(Key : String; JsonValue : String) : String;
Begin
  Result := JStr(Key) + ':' + JsonValue;
End;

Function Join(A : String; B : String) : String;
Begin
  If A = '' Then Result := B
  Else If B = '' Then Result := A
  Else Result := A + ',' + B;
End;

{ ---------- job plumbing ---------- }

Procedure Log(S : String);
Begin
  Stage := S;
  LogBuf.Add(FormatDateTime('hh:nn:ss', Now) + ' ' + S);
  Try
    LogBuf.SaveToFile(JobDir + 'log.txt');
  Except
  End;
End;

Function P(Key : String) : String;
Begin
  Result := Req.Values[Key];
End;

Function PInt(Key : String; Dflt : Integer) : Integer;
Begin
  Result := StrToIntDef(Trim(P(Key)), Dflt);
End;

Function PBool(Key : String) : Boolean;
Begin
  Result := (P(Key) = '1') Or (LowerCase(P(Key)) = 'true');
End;

Function Has(Key : String) : Boolean;
Begin
  Result := Req.IndexOfName(Key) >= 0;
End;

{ Decimal number with a dot, independent of the locale ("-12.5"). }
Function ParseNum(S : String) : Double;
Var I, Dot : Integer; Neg : Boolean; IntPart, Frac : String; R, Scale : Double;
Begin
  S := Trim(S);
  Neg := False;
  If Copy(S, 1, 1) = '-' Then
  Begin
    Neg := True;
    S := Copy(S, 2, Length(S));
  End;
  Dot := Pos('.', S);
  If Dot > 0 Then
  Begin
    IntPart := Copy(S, 1, Dot - 1);
    Frac := Copy(S, Dot + 1, Length(S));
  End
  Else
  Begin
    IntPart := S;
    Frac := '';
  End;
  If IntPart = '' Then IntPart := '0';
  R := StrToIntDef(IntPart, 0);
  Scale := 0.1;
  For I := 1 To Length(Frac) Do
  Begin
    R := R + StrToIntDef(Copy(Frac, I, 1), 0) * Scale;
    Scale := Scale / 10;
  End;
  If Neg Then R := -R;
  Result := R;
End;

{ Items of a list parameter: KEY_count plus KEY1..KEYn. }
Function ListCount(Key : String) : Integer;
Begin
  Result := PInt(Key + '_count', 0);
End;

Function ListItem(Key : String; I : Integer) : String;
Begin
  Result := P(Key + IntToStr(I));
End;

Procedure Finish(Ok : Boolean; Err : String);
Var F : TStringList;
Begin
  F := TStringList.Create;
  If Ok Then
    F.Text := '{"ok":true,"data":{' + Data + '}}'
  Else
    F.Text := '{"ok":false,"error":' + JStr(Err) + ',"data":{' + Data + '}}';
  F.SaveToFile(JobDir + 'response.json');
  F.Clear;
  F.SaveToFile(JobDir + 'done');
  F.Free;
End;

Procedure Fail(Err : String);
Begin
  Failed := True;
  If Not FileExists(JobDir + 'done') Then Finish(False, Err);
End;

{ ---------- workspace helpers ---------- }

Procedure RunWithParams(Process : String; Params : String);
Var L : TStringList; I : Integer;
Begin
  ResetParameters;
  L := TStringList.Create;
  L.Delimiter := '|';
  L.StrictDelimiter := True;
  L.DelimitedText := Params;
  For I := 0 To L.Count - 1 Do
    If L.Names[I] <> '' Then AddStringParameter(L.Names[I], L.ValueFromIndex[I]);
  L.Free;
  RunProcess(Process);
End;

Procedure OpenObject(Kind : String; Path : String);
Begin
  ResetParameters;
  AddStringParameter('ObjectKind', Kind);
  AddStringParameter('FileName', Path);
  RunProcess('WorkspaceManager:OpenObject');
End;

Function FindProject(Path : String) : IProject;
Var WS : IWorkspace; I : Integer; Prj : IProject;
Begin
  Result := Nil;
  WS := GetWorkspace;
  For I := 0 To WS.DM_ProjectCount - 1 Do
  Begin
    Prj := WS.DM_Projects(I);
    If UpperCase(Prj.DM_ProjectFullPath) = UpperCase(Path) Then
    Begin
      Result := Prj;
      Exit;
    End;
  End;
End;

Function OpenProject(Path : String) : IProject;
Begin
  Result := FindProject(Path);
  If Result = Nil Then
  Begin
    Log('opening project ' + Path);
    OpenObject('Project', Path);
    Result := FindProject(Path);
  End;
End;

Function NeedProject : IProject;
Begin
  Result := OpenProject(P('project'));
  If Result = Nil Then Fail('project could not be opened: ' + P('project'));
End;

Function ProjectsJson : String;
Var WS : IWorkspace; I : Integer; S : String;
Begin
  WS := GetWorkspace;
  S := '';
  For I := 0 To WS.DM_ProjectCount - 1 Do
    S := Join(S, JStr(WS.DM_Projects(I).DM_ProjectFullPath));
  Result := '[' + S + ']';
End;

Function DocumentsJson(Prj : IProject) : String;
Var I : Integer; D : IDocument; S : String;
Begin
  S := '';
  For I := 0 To Prj.DM_LogicalDocumentCount - 1 Do
  Begin
    D := Prj.DM_LogicalDocuments(I);
    S := Join(S, '{"path":' + JStr(D.DM_FullPath) + ',"kind":' + JStr(D.DM_DocumentKind) + '}');
  End;
  Result := '[' + S + ']';
End;

{ Open a document in an editor (needed before PCBServer/SchServer can see it). }
Function OpenDoc(Kind : String; Path : String) : IServerDocument;
Begin
  Result := Client.OpenDocument(Kind, Path);
  If Result <> Nil Then Client.ShowDocument(Result);
End;

Procedure SaveDoc(Path : String);
Var SD : IServerDocument;
Begin
  SD := Client.GetDocumentByPath(Path);
  If SD <> Nil Then
  Begin
    Log('saving ' + Path);
    SD.DoFileSave('');
  End;
End;

Function NeedBoard : IPCB_Board;
Begin
  Log('open pcb ' + P('pcb'));
  OpenDoc('PCB', P('pcb'));
  Result := PCBServer.GetPCBBoardByPath(P('pcb'));
  If Result = Nil Then Fail('PCB could not be opened: ' + P('pcb'));
End;

Function NeedSheet : ISch_Document;
Begin
  Log('open schematic ' + P('sch'));
  OpenDoc('SCH', P('sch'));
  Result := SchServer.GetSchDocumentByPath(P('sch'));
  If Result = Nil Then Fail('schematic could not be opened: ' + P('sch'));
End;

{ ---------- design-data JSON ---------- }

Function ParamsJson(O : IDMObject) : String;
Var I : Integer; Prm : IParameter; S : String;
Begin
  S := '';
  For I := 0 To O.DM_ParameterCount - 1 Do
  Begin
    Prm := O.DM_Parameters(I);
    S := Join(S, JStr(Prm.DM_Name) + ':' + JStr(Prm.DM_Value));
  End;
  Result := '{' + S + '}';
End;

Function DMObjectJson(O : IDMObject) : String;
Var S : String;
Begin
  S := Member('kind', JStr(O.DM_ObjectKindString));
  S := Join(S, Member('descriptor', JStr(O.DM_LongDescriptorString)));
  S := Join(S, Member('document', JStr(O.DM_OwnerDocumentFullPath)));
  If O.DM_LocationX <> -9999 Then
  Begin
    S := Join(S, Member('x', JC(O.DM_LocationX)));
    S := Join(S, Member('y', JC(O.DM_LocationY)));
  End;
  Result := '{' + S + '}';
End;

Function LevelName(L : Integer) : String;
Begin
  If L = 0 Then Result := 'no_report'
  Else If L = 1 Then Result := 'warning'
  Else If L = 2 Then Result := 'error'
  Else If L = 3 Then Result := 'fatal'
  Else Result := IntToStr(L);
End;

Function ViolationsJson(Prj : IProject) : String;
Var I, K : Integer; V : IViolation; S, Item, Objs : String;
Begin
  S := '';
  For I := 0 To Prj.DM_ViolationCount - 1 Do
  Begin
    V := Prj.DM_Violations(I);
    Item := Member('level', JInt(V.DM_ErrorLevel));
    Item := Join(Item, Member('severity', JStr(LevelName(V.DM_ErrorLevel))));
    Item := Join(Item, Member('kind', JInt(V.DM_ErrorKind)));
    Item := Join(Item, Member('descriptor', JStr(V.DM_DescriptorString)));
    Item := Join(Item, Member('detail', JStr(V.DM_DetailString)));
    Objs := '';
    For K := 0 To V.DM_RelatedObjectCount - 1 Do
      Objs := Join(Objs, DMObjectJson(V.DM_RelatedObjects(K)));
    Item := Join(Item, Member('objects', '[' + Objs + ']'));
    S := Join(S, '{' + Item + '}');
  End;
  Result := '[' + S + ']';
End;

{ ---------- operations: workspace ---------- }

Procedure OpPing;
Begin
  Put('protocol', JStr('2'));
  Put('product_version', JStr(Client.GetProductVersion));
  Put('projects', ProjectsJson);
  If GetWorkspace.DM_FocusedDocument <> Nil Then
    Put('focused_document', JStr(GetWorkspace.DM_FocusedDocument.DM_FullPath));
End;

Procedure OpDocuments;
Var WS : IWorkspace; I : Integer; S : String; Prj : IProject;
Begin
  WS := GetWorkspace;
  S := '';
  For I := 0 To WS.DM_ProjectCount - 1 Do
  Begin
    Prj := WS.DM_Projects(I);
    S := Join(S, '{"project":' + JStr(Prj.DM_ProjectFullPath) + ',"documents":' + DocumentsJson(Prj) + '}');
  End;
  Put('projects', '[' + S + ']');
End;

Procedure OpOpen;
Var Path, Kind : String; Prj : IProject;
Begin
  Path := P('path');
  Kind := P('kind');
  If Kind = '' Then Kind := 'Document';
  Log('open ' + Kind + ' ' + Path);
  OpenObject(Kind, Path);
  Put('opened', JStr(Path));
  If Kind = 'Project' Then
  Begin
    Prj := FindProject(Path);
    If Prj <> Nil Then Put('documents', DocumentsJson(Prj));
  End;
End;

Procedure OpCompile;
Var Prj : IProject; I, Errors, Warnings, L : Integer;
Begin
  Prj := NeedProject;
  If Failed Then Exit;
  Log('compiling');
  Prj.DM_Compile;
  Log('collecting violations');
  Put('project', JStr(Prj.DM_ProjectFullPath));
  Put('documents', DocumentsJson(Prj));
  Put('violations', ViolationsJson(Prj));
  Errors := 0;
  Warnings := 0;
  For I := 0 To Prj.DM_ViolationCount - 1 Do
  Begin
    L := Prj.DM_Violations(I).DM_ErrorLevel;
    If L >= 2 Then Errors := Errors + 1
    Else If L = 1 Then Warnings := Warnings + 1;
  End;
  Put('errors', JInt(Errors));
  Put('warnings', JInt(Warnings));
End;

{ Compiled (flattened) design as Altium sees it: components with parameters, nets with pins. }
Procedure OpNetlist;
Var Prj : IProject; Flat : IDocument; I, K : Integer; C : IComponent; N : INet; Pin : IPin;
    S, Item, Pins : String;
Begin
  Prj := NeedProject;
  If Failed Then Exit;
  Log('compiling');
  Prj.DM_Compile;
  Flat := Prj.DM_DocumentFlattened;
  If Flat = Nil Then
  Begin
    Fail('the project has no compiled (flattened) document; does it contain schematics?');
    Exit;
  End;
  Log('reading components');
  S := '';
  For I := 0 To Flat.DM_ComponentCount - 1 Do
  Begin
    C := Flat.DM_Components(I);
    Item := Member('designator', JStr(C.DM_PhysicalDesignator));
    Item := Join(Item, Member('comment', JStr(C.DM_Comment)));
    Item := Join(Item, Member('footprint', JStr(C.DM_Footprint)));
    Item := Join(Item, Member('lib_ref', JStr(C.DM_LibraryReference)));
    Item := Join(Item, Member('library', JStr(C.DM_SourceLibraryName)));
    Item := Join(Item, Member('unique_id', JStr(C.DM_UniqueId)));
    Item := Join(Item, Member('parameters', ParamsJson(C)));
    S := Join(S, '{' + Item + '}');
  End;
  Put('components', '[' + S + ']');
  Log('reading nets');
  S := '';
  For I := 0 To Flat.DM_NetCount - 1 Do
  Begin
    N := Flat.DM_Nets(I);
    Pins := '';
    For K := 0 To N.DM_PinCount - 1 Do
    Begin
      Pin := N.DM_Pins(K);
      Pins := Join(Pins, '{"designator":' + JStr(Pin.DM_PhysicalPartDesignator) + ',"pin":' + JStr(Pin.DM_PinNumber) +
                         ',"name":' + JStr(Pin.DM_PinName) + '}');
    End;
    S := Join(S, '{"name":' + JStr(N.DM_NetName) + ',"pins":[' + Pins + ']}');
  End;
  Put('nets', '[' + S + ']');
  Put('project', JStr(Prj.DM_ProjectFullPath));
End;

Function VariationKindName(K : Integer) : String;
Begin
  If K = 0 Then Result := 'fitted'
  Else If K = 1 Then Result := 'not_fitted'
  Else If K = 2 Then Result := 'alternate'
  Else Result := IntToStr(K);
End;

Procedure OpVariants;
Var Prj : IProject; I, K : Integer; V : IProjectVariant; CV : IComponentVariation; S, Items : String;
Begin
  Prj := NeedProject;
  If Failed Then Exit;
  S := '';
  For I := 0 To Prj.DM_ProjectVariantCount - 1 Do
  Begin
    V := Prj.DM_ProjectVariants(I);
    Items := '';
    For K := 0 To V.DM_VariationCount - 1 Do
    Begin
      CV := V.DM_Variations(K);
      Items := Join(Items, '{"designator":' + JStr(CV.DM_PhysicalDesignator) + ',"kind":' +
                           JStr(VariationKindName(CV.DM_VariationKind)) + '}');
    End;
    S := Join(S, '{"name":' + JStr(V.DM_Description) + ',"variations":[' + Items + ']}');
  End;
  Put('variants', '[' + S + ']');
  If Prj.DM_CurrentProjectVariant <> Nil Then
    Put('current', JStr(Prj.DM_CurrentProjectVariant.DM_Description))
  Else
    Put('current', 'null');
End;

{ Designators of the compiled design, keyed by unique id, for before/after reports. }
Function DesignatorList(Prj : IProject) : TStringList;
Var Flat : IDocument; I : Integer; C : IComponent;
Begin
  Result := TStringList.Create;
  Prj.DM_Compile;
  Flat := Prj.DM_DocumentFlattened;
  If Flat = Nil Then Exit;
  For I := 0 To Flat.DM_ComponentCount - 1 Do
  Begin
    C := Flat.DM_Components(I);
    Result.Add(C.DM_UniqueId + '=' + C.DM_PhysicalDesignator);
  End;
End;

{ annotate: mode = quiet | all | reset | reset_duplicates | board | back }
Procedure OpAnnotate;
Var Prj : IProject; Before, After : TStringList; I : Integer; Mode, Action, S, Was : String;
Begin
  Prj := NeedProject;
  If Failed Then Exit;
  Mode := P('mode');
  If Mode = '' Then Mode := 'quiet';
  If Mode = 'quiet' Then Action := 'AnnotateQuiet'
  Else If Mode = 'all' Then Action := 'AnnotateAll'
  Else If Mode = 'reset' Then Action := 'AnnotateReset'
  Else If Mode = 'reset_duplicates' Then Action := 'AnnotateResetDup'
  Else If Mode = 'board' Then Action := 'AnnotatePhysical'
  Else If Mode = 'back' Then Action := 'BackAnnotate'
  Else
  Begin
    Fail('unknown annotate mode: ' + Mode);
    Exit;
  End;
  Before := DesignatorList(Prj);
  Log('running ' + Action);
  Prj.DM_SetAsCurrentProject;
  ResetParameters;
  AddStringParameter('ObjectKind', 'Project');
  AddStringParameter('Action', Action);
  RunProcess('WorkspaceManager:DocumentOptions');
  After := DesignatorList(Prj);
  S := '';
  For I := 0 To After.Count - 1 Do
  Begin
    Was := Before.Values[After.Names[I]];
    If Was <> After.ValueFromIndex[I] Then
      S := Join(S, '{"before":' + JStr(Was) + ',"after":' + JStr(After.ValueFromIndex[I]) + '}');
  End;
  For I := 0 To Prj.DM_LogicalDocumentCount - 1 Do SaveDoc(Prj.DM_LogicalDocuments(I).DM_FullPath);
  Put('mode', JStr(Mode));
  Put('changes', '[' + S + ']');
  Before.Free;
  After.Free;
End;

Procedure GenerateMedium(Medium : String; Action : String);
Begin
  If Action = '' Then Action := 'Run';
  Log('generating ' + Medium + ' (' + Action + ')');
  ResetParameters;
  AddStringParameter('Action', Action);
  AddStringParameter('ObjectKind', 'OutputBatch');
  If Medium <> '' Then AddStringParameter('OutputMedium', Medium);
  If Action = 'PublishToPDF' Then AddStringParameter('DisableDialog', 'True');
  RunProcess('WorkspaceManager:GenerateReport');
End;

{ outjob: project, outjob; one medium (medium, action) or a list (m1..mN with a1..aN);
  temporary=1 adds the OutJob to the project for the run and removes it afterwards. }
Procedure OpOutjob;
Var Prj : IProject; Added : Boolean; I : Integer; SD : IServerDocument; Done : String;
Begin
  Prj := Nil;
  Added := False;
  If P('project') <> '' Then
  Begin
    Prj := NeedProject;
    If Failed Then Exit;
  End;
  If (Prj <> Nil) And PBool('temporary') Then
  Begin
    Log('adding output job to the project (temporarily)');
    Prj.DM_AddSourceDocument(P('outjob'));
    Added := True;
  End;
  Log('open outjob ' + P('outjob'));
  SD := OpenDoc('OUTPUTJOB', P('outjob'));
  Done := '';
  If ListCount('m') > 0 Then
  Begin
    For I := 1 To ListCount('m') Do
    Begin
      GenerateMedium(ListItem('m', I), ListItem('a', I));
      Done := Join(Done, JStr(ListItem('m', I)));
    End;
  End
  Else
  Begin
    GenerateMedium(P('medium'), P('action'));
    Done := JStr(P('medium'));
  End;
  Log('generated');
  If Added Then
  Begin
    SD := Client.GetDocumentByPath(P('outjob'));
    If SD <> Nil Then Client.CloseDocument(SD);
    Prj.DM_RemoveSourceDocument(P('outjob'));
    Log('removed output job from the project');
  End;
  Put('outjob', JStr(P('outjob')));
  Put('mediums', '[' + Done + ']');
  If Prj <> Nil Then Put('output_path', JStr(Prj.DM_GetOutputPath));
End;

{ process: name=Server:Process, params=Key=Value|Key=Value (pipe separated) }
Procedure OpProcess;
Begin
  Log('RunProcess ' + P('name') + ' ' + P('params'));
  RunWithParams(P('name'), P('params'));
  Put('process', JStr(P('name')));
  Put('params', JStr(P('params')));
End;

Procedure OpSaveAll;
Begin
  ResetParameters;
  AddStringParameter('ObjectKind', 'All');
  RunProcess('WorkspaceManager:SaveObject');
  Put('saved', 'true');
End;

Procedure OpCloseAll;
Begin
  ResetParameters;
  AddStringParameter('ObjectKind', 'All');
  RunProcess('WorkspaceManager:CloseObject');
  Put('closed', 'true');
End;

{ ---------- PCB ---------- }

Function RectJson(R : TCoordRect) : String;
Begin
  Result := '{"left":' + JC(R.Left) + ',"bottom":' + JC(R.Bottom) + ',"right":' + JC(R.Right) + ',"top":' + JC(R.Top) + '}';
End;

Function PrimJson(Prim : IPCB_Primitive) : String;
Var S : String;
Begin
  If Prim = Nil Then
  Begin
    Result := 'null';
    Exit;
  End;
  S := Member('object', JStr(Prim.ObjectIDString));
  S := Join(S, Member('descriptor', JStr(Prim.Descriptor)));
  S := Join(S, Member('layer', JStr(Layer2String(Prim.Layer))));
  If Prim.InNet Then S := Join(S, Member('net', JStr(Prim.Net.Name)));
  S := Join(S, Member('bbox', RectJson(Prim.BoundingRectangle)));
  Result := '{' + S + '}';
End;

Function NewIterator(Board : IPCB_Board; Kind : Integer) : IPCB_BoardIterator;
Begin
  Result := Board.BoardIterator_Create;
  Result.AddFilter_ObjectSet(MkSet(Kind));
  Result.AddFilter_LayerSet(AllLayers);
  Result.AddFilter_Method(eProcessAll);
End;

Function CountObjects(Board : IPCB_Board; Kind : Integer) : Integer;
Var It : IPCB_BoardIterator; O : IPCB_Primitive;
Begin
  Result := 0;
  It := NewIterator(Board, Kind);
  O := It.FirstPCBObject;
  While O <> Nil Do
  Begin
    Result := Result + 1;
    O := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
End;

Function ViolationsOfBoard(Board : IPCB_Board; Var N : Integer) : String;
Var It : IPCB_BoardIterator; V : IPCB_Violation; S, Item : String;
Begin
  S := '';
  N := 0;
  It := NewIterator(Board, eViolationObject);
  V := It.FirstPCBObject;
  While V <> Nil Do
  Begin
    Item := Member('name', JStr(V.Name));
    Item := Join(Item, Member('description', JStr(V.Description)));
    If V.Rule <> Nil Then
    Begin
      Item := Join(Item, Member('rule', JStr(V.Rule.Name)));
      Item := Join(Item, Member('rule_kind', JInt(V.Rule.RuleKind)));
    End;
    Item := Join(Item, Member('layer', JStr(Layer2String(V.Layer))));
    Item := Join(Item, Member('bbox', RectJson(V.BoundingRectangle)));
    Item := Join(Item, Member('primitive1', PrimJson(V.Primitive1)));
    Item := Join(Item, Member('primitive2', PrimJson(V.Primitive2)));
    S := Join(S, '{' + Item + '}');
    N := N + 1;
    V := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  Result := '[' + S + ']';
End;

Procedure OpDrc;
Var Board : IPCB_Board; N : Integer; Proc, Params : String;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  Proc := P('process');
  If Proc = '' Then Proc := 'PCB:DesignRuleCheck';
  Params := P('params');
  Log('running ' + Proc + ' ' + Params);
  RunWithParams(Proc, Params);
  Log('collecting violations');
  Put('pcb', JStr(Board.FileName));
  Put('violations', ViolationsOfBoard(Board, N));
  Put('count', JInt(N));
End;

Function LayerSide(L : Integer) : String;
Begin
  If L = eBottomLayer Then Result := 'bottom' Else Result := 'top';
End;

Function ComponentJson(C : IPCB_Component) : String;
Var S : String;
Begin
  S := Member('designator', JStr(C.Name.Text));
  S := Join(S, Member('comment', JStr(C.Comment.Text)));
  S := Join(S, Member('footprint', JStr(C.Pattern)));
  S := Join(S, Member('x', JC(C.X)));
  S := Join(S, Member('y', JC(C.Y)));
  S := Join(S, Member('rotation', JF(C.Rotation)));
  S := Join(S, Member('side', JStr(LayerSide(C.Layer))));
  S := Join(S, Member('locked', JBool(Not C.Moveable)));
  S := Join(S, Member('selected', JBool(C.Selected)));
  S := Join(S, Member('bbox', RectJson(C.BoundingRectangle)));
  Result := '{' + S + '}';
End;

Function FindComponent(Board : IPCB_Board; Designator : String) : IPCB_Component;
Var It : IPCB_BoardIterator; C : IPCB_Component;
Begin
  Result := Nil;
  It := NewIterator(Board, eComponentObject);
  C := It.FirstPCBObject;
  While C <> Nil Do
  Begin
    If UpperCase(C.Name.Text) = UpperCase(Designator) Then
    Begin
      Result := C;
      Break;
    End;
    C := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
End;

Procedure OpPcbComponents;
Var Board : IPCB_Board; It : IPCB_BoardIterator; C : IPCB_Component; S : String;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  S := '';
  It := NewIterator(Board, eComponentObject);
  C := It.FirstPCBObject;
  While C <> Nil Do
  Begin
    S := Join(S, ComponentJson(C));
    C := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  Put('pcb', JStr(Board.FileName));
  Put('components', '[' + S + ']');
End;

{ pcb_edit: list "d" of designators; per item i optional xI, yI (absolute, internal units),
  dxI, dyI (relative), rotI (degrees), sideI (top|bottom), lockI (0|1).
  dry_run=1 reports the change without making it. }
Procedure OpPcbEdit;
Var Board : IPCB_Board; I, N : Integer; C : IPCB_Component; S, Item, D, K : String;
    NX, NY : Integer; Dry, Changed : Boolean;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  Dry := PBool('dry_run');
  N := ListCount('d');
  S := '';
  If Not Dry Then PCBServer.PreProcess;
  For I := 1 To N Do
  Begin
    D := ListItem('d', I);
    K := IntToStr(I);
    C := FindComponent(Board, D);
    If C = Nil Then
    Begin
      If Not Dry Then PCBServer.PostProcess;
      Fail('no component ' + D + ' on the PCB');
      Exit;
    End;
    Item := Member('before', ComponentJson(C));
    Changed := False;
    If Not Dry Then
      PCBServer.SendMessageToRobots(C.I_ObjectAddress, c_Broadcast, PCBM_BeginModify, c_NoEventData);
    If Has('side' + K) Then
      If ((P('side' + K) = 'bottom') And (C.Layer <> eBottomLayer)) Or ((P('side' + K) = 'top') And (C.Layer = eBottomLayer)) Then
      Begin
        Changed := True;
        If Not Dry Then C.FlipComponent;
      End;
    If Has('rot' + K) Then
    Begin
      Changed := True;
      If Not Dry Then C.Rotation := ParseNum(P('rot' + K));
    End;
    If Has('x' + K) Or Has('y' + K) Or Has('dx' + K) Or Has('dy' + K) Then
    Begin
      NX := C.X;
      NY := C.Y;
      If Has('x' + K) Then NX := PInt('x' + K, NX);
      If Has('y' + K) Then NY := PInt('y' + K, NY);
      NX := NX + PInt('dx' + K, 0);
      NY := NY + PInt('dy' + K, 0);
      Changed := True;
      If Not Dry Then C.MoveToXY(NX, NY);
    End;
    If Has('lock' + K) Then
    Begin
      Changed := True;
      If Not Dry Then C.Moveable := Not PBool('lock' + K);
    End;
    If Not Dry Then
    Begin
      PCBServer.SendMessageToRobots(C.I_ObjectAddress, c_Broadcast, PCBM_EndModify, c_NoEventData);
      Item := Join(Item, Member('after', ComponentJson(C)));
    End;
    Item := Join(Item, Member('changed', JBool(Changed)));
    S := Join(S, '{' + Item + '}');
  End;
  If Not Dry Then
  Begin
    PCBServer.PostProcess;
    Board.ViewManager_FullUpdate;
    If Not PBool('no_save') Then SaveDoc(P('pcb'));
  End;
  Put('dry_run', JBool(Dry));
  Put('items', '[' + S + ']');
End;

Function PolygonJson(Poly : IPCB_Polygon) : String;
Var S : String;
Begin
  S := Member('name', JStr(Poly.Name));
  S := Join(S, Member('layer', JStr(Layer2String(Poly.Layer))));
  If Poly.InNet Then S := Join(S, Member('net', JStr(Poly.Net.Name)));
  S := Join(S, Member('poured', JBool(Poly.Poured)));
  S := Join(S, Member('enabled', JBool(Poly.Enabled)));
  S := Join(S, Member('bbox', RectJson(Poly.BoundingRectangle)));
  Result := '{' + S + '}';
End;

Function PolygonsJson(Board : IPCB_Board) : String;
Var It : IPCB_BoardIterator; Poly : IPCB_Polygon; S : String;
Begin
  S := '';
  It := NewIterator(Board, ePolyObject);
  Poly := It.FirstPCBObject;
  While Poly <> Nil Do
  Begin
    S := Join(S, PolygonJson(Poly));
    Poly := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  Result := '[' + S + ']';
End;

{ pcb_polygons: action = list | repour | repour_modified | shelve | restore }
Procedure OpPcbPolygons;
Var Board : IPCB_Board; Action : String;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  Action := P('action');
  If Action = '' Then Action := 'list';
  Put('before', PolygonsJson(Board));
  If Action <> 'list' Then
  Begin
    Log('polygons: ' + Action);
    If Action = 'repour' Then RunWithParams('PCB:ChangeObject', 'Action=RepourAllPolygons')
    Else If Action = 'repour_modified' Then RunWithParams('PCB:ChangeObject', 'Action=RepourModified')
    Else If Action = 'shelve' Then RunWithParams('PCB:ShelvePolygons', 'Shelve=True')
    Else If Action = 'restore' Then RunWithParams('PCB:ShelvePolygons', '')
    Else
    Begin
      Fail('unknown polygon action: ' + Action);
      Exit;
    End;
    Board.ViewManager_FullUpdate;
    Put('after', PolygonsJson(Board));
    If Not PBool('no_save') Then SaveDoc(P('pcb'));
  End;
  Put('action', JStr(Action));
End;

Function RuleJson(R : IPCB_Rule) : String;
Var S : String;
Begin
  S := Member('name', JStr(R.Name));
  S := Join(S, Member('kind', JInt(R.RuleKind)));
  S := Join(S, Member('enabled', JBool(R.Enabled)));
  S := Join(S, Member('priority', JInt(R.Priority)));
  S := Join(S, Member('scope1', JStr(R.Scope1Expression)));
  S := Join(S, Member('scope2', JStr(R.Scope2Expression)));
  S := Join(S, Member('comment', JStr(R.Comment)));
  S := Join(S, Member('descriptor', JStr(R.Descriptor)));
  Result := '{' + S + '}';
End;

Function FindRule(Board : IPCB_Board; Name : String) : IPCB_Rule;
Var It : IPCB_BoardIterator; R : IPCB_Rule;
Begin
  Result := Nil;
  It := NewIterator(Board, eRuleObject);
  R := It.FirstPCBObject;
  While R <> Nil Do
  Begin
    If UpperCase(R.Name) = UpperCase(Name) Then
    Begin
      Result := R;
      Break;
    End;
    R := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
End;

Procedure OpPcbRules;
Var Board : IPCB_Board; It : IPCB_BoardIterator; R : IPCB_Rule; S : String;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  S := '';
  It := NewIterator(Board, eRuleObject);
  R := It.FirstPCBObject;
  While R <> Nil Do
  Begin
    S := Join(S, RuleJson(R));
    R := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  Put('rules', '[' + S + ']');
End;

{ pcb_rule: name; kind (clearance | width | ...) for a new rule; scope1, scope2, priority,
  enabled, comment; values: gap (clearance), min/max/preferred (width); delete=1. }
Procedure OpPcbRule;
Var Board : IPCB_Board; R : IPCB_Rule; Kind : String; IsNew, Dry : Boolean;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  Dry := PBool('dry_run');
  R := FindRule(Board, P('name'));
  If R <> Nil Then Put('before', RuleJson(R)) Else Put('before', 'null');
  If PBool('delete') Then
  Begin
    If R = Nil Then
    Begin
      Fail('no rule named ' + P('name'));
      Exit;
    End;
    If Not Dry Then
    Begin
      Board.RemovePCBObject(R);
      If Not PBool('no_save') Then SaveDoc(P('pcb'));
    End;
    Put('deleted', 'true');
    Put('dry_run', JBool(Dry));
    Exit;
  End;
  IsNew := R = Nil;
  If IsNew Then
  Begin
    Kind := LowerCase(P('kind'));
    If Kind = 'clearance' Then R := PCBServer.PCBRuleFactory(eRule_Clearance)
    Else If Kind = 'width' Then R := PCBServer.PCBRuleFactory(eRule_MaxMinWidth)
    Else If Kind = 'hole_size' Then R := PCBServer.PCBRuleFactory(eRule_MaxMinHoleSize)
    Else If Kind = 'via_style' Then R := PCBServer.PCBRuleFactory(eRule_RoutingViaStyle)
    Else If Kind = 'short_circuit' Then R := PCBServer.PCBRuleFactory(eRule_ShortCircuit)
    Else If Kind = 'unrouted' Then R := PCBServer.PCBRuleFactory(eRule_BrokenNets)
    Else If Kind = 'routing_layers' Then R := PCBServer.PCBRuleFactory(eRule_RoutingLayers)
    Else If Kind = 'routing_priority' Then R := PCBServer.PCBRuleFactory(eRule_RoutingPriority)
    Else If Kind = 'differential_pairs' Then R := PCBServer.PCBRuleFactory(eRule_DifferentialPairsRouting)
    Else If Kind = 'length' Then R := PCBServer.PCBRuleFactory(eRule_MaxMinLength)
    Else If Kind = 'matched_lengths' Then R := PCBServer.PCBRuleFactory(eRule_MatchedLengths)
    Else If Kind = 'component_clearance' Then R := PCBServer.PCBRuleFactory(eRule_ComponentClearance)
    Else If Kind = 'hole_to_hole' Then R := PCBServer.PCBRuleFactory(eRule_HoleToHoleClearance)
    Else If Kind = 'solder_mask' Then R := PCBServer.PCBRuleFactory(eRule_SolderMaskExpansion)
    Else If Kind = 'paste_mask' Then R := PCBServer.PCBRuleFactory(eRule_PasteMaskExpansion)
    Else If Kind = 'polygon_connect' Then R := PCBServer.PCBRuleFactory(eRule_PolygonConnectStyle)
    Else If Kind = 'plane_connect' Then R := PCBServer.PCBRuleFactory(eRule_PowerPlaneConnectStyle)
    Else
    Begin
      Fail('unknown rule kind for a new rule: ' + P('kind'));
      Exit;
    End;
    R.Name := P('name');
  End;
  If Dry Then
  Begin
    Put('created', JBool(IsNew));
    Put('dry_run', 'true');
    Exit;
  End;
  If Has('scope1') Then R.Scope1Expression := P('scope1');
  If Has('scope2') Then R.Scope2Expression := P('scope2');
  If IsNew And Not Has('scope1') Then R.Scope1Expression := 'All';
  If IsNew And Not Has('scope2') Then R.Scope2Expression := 'All';
  If Has('priority') Then R.Priority := PInt('priority', 1);
  If Has('enabled') Then R.Enabled := PBool('enabled');
  If Has('comment') Then R.Comment := P('comment');
  If R.RuleKind = eRule_Clearance Then
  Begin
    If Has('gap') Then R.Gap := PInt('gap', 0);
  End
  Else If R.RuleKind = eRule_MaxMinWidth Then
  Begin
    If Has('min') Then R.MinWidth[eTopLayer] := PInt('min', 0);
    If Has('max') Then R.MaxWidth[eTopLayer] := PInt('max', 0);
    If Has('preferred') Then R.FavoredWidth[eTopLayer] := PInt('preferred', 0);
  End
  Else If R.RuleKind = eRule_MaxMinHoleSize Then
  Begin
    If Has('min') Then R.MinLimit := PInt('min', 0);
    If Has('max') Then R.MaxLimit := PInt('max', 0);
  End;
  If IsNew Then Board.AddPCBObject(R);
  Put('created', JBool(IsNew));
  Put('after', RuleJson(R));
  Put('dry_run', 'false');
  If Not PBool('no_save') Then SaveDoc(P('pcb'));
End;

Function ClassKindName(K : Integer) : String;
Begin
  If K = eClassMemberKind_Net Then Result := 'net'
  Else If K = eClassMemberKind_Component Then Result := 'component'
  Else If K = eClassMemberKind_Layer Then Result := 'layer'
  Else If K = eClassMemberKind_Pad Then Result := 'pad'
  Else If K = eClassMemberKind_DifferentialPair Then Result := 'differential_pair'
  Else If K = eClassMemberKind_Polygon Then Result := 'polygon'
  Else Result := IntToStr(K);
End;

Function ClassJson(Cl : IPCB_ObjectClass; WithMembers : Boolean) : String;
Var S, M : String; I : Integer;
Begin
  S := Member('name', JStr(Cl.Name));
  S := Join(S, Member('kind', JStr(ClassKindName(Cl.MemberKind))));
  S := Join(S, Member('builtin', JBool(Cl.SuperClass)));
  If WithMembers Then
  Begin
    M := '';
    For I := 0 To Cl.MembersCount - 1 Do
      M := Join(M, JStr(Cl.MemberName(I)));
    S := Join(S, Member('members', '[' + M + ']'));
  End;
  Result := '{' + S + '}';
End;

Function FindClass(Board : IPCB_Board; Name : String; Kind : Integer) : IPCB_ObjectClass;
Var It : IPCB_BoardIterator; Cl : IPCB_ObjectClass;
Begin
  Result := Nil;
  It := NewIterator(Board, eClassObject);
  Cl := It.FirstPCBObject;
  While Cl <> Nil Do
  Begin
    If (UpperCase(Cl.Name) = UpperCase(Name)) And (Cl.MemberKind = Kind) Then
    Begin
      Result := Cl;
      Break;
    End;
    Cl := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
End;

Function ClassKindOf(S : String) : Integer;
Begin
  S := LowerCase(S);
  If S = 'component' Then Result := eClassMemberKind_Component
  Else If S = 'layer' Then Result := eClassMemberKind_Layer
  Else If S = 'pad' Then Result := eClassMemberKind_Pad
  Else If S = 'differential_pair' Then Result := eClassMemberKind_DifferentialPair
  Else Result := eClassMemberKind_Net;
End;

{ pcb_classes: list (name empty) or edit class NAME of KIND: add list "add", remove list
  "remove", delete=1. }
Procedure OpPcbClasses;
Var Board : IPCB_Board; It : IPCB_BoardIterator; Cl : IPCB_ObjectClass; S : String; Kind, I : Integer;
    IsNew, Dry : Boolean;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  If P('name') = '' Then
  Begin
    S := '';
    It := NewIterator(Board, eClassObject);
    Cl := It.FirstPCBObject;
    While Cl <> Nil Do
    Begin
      S := Join(S, ClassJson(Cl, True));
      Cl := It.NextPCBObject;
    End;
    Board.BoardIterator_Destroy(It);
    Put('classes', '[' + S + ']');
    Exit;
  End;
  Dry := PBool('dry_run');
  Kind := ClassKindOf(P('kind'));
  Cl := FindClass(Board, P('name'), Kind);
  If Cl <> Nil Then Put('before', ClassJson(Cl, True)) Else Put('before', 'null');
  Put('dry_run', JBool(Dry));
  If Dry Then Exit;
  If PBool('delete') Then
  Begin
    If Cl = Nil Then
    Begin
      Fail('no class named ' + P('name'));
      Exit;
    End;
    Board.RemovePCBObject(Cl);
    Put('deleted', 'true');
    If Not PBool('no_save') Then SaveDoc(P('pcb'));
    Exit;
  End;
  IsNew := Cl = Nil;
  PCBServer.PreProcess;
  If IsNew Then
  Begin
    Cl := PCBServer.PCBClassFactoryByClassMember(Kind);
    Cl.SuperClass := False;
    Cl.Name := P('name');
    Board.AddPCBObject(Cl);
  End;
  For I := 1 To ListCount('add') Do Cl.AddMemberByName(ListItem('add', I));
  For I := 1 To ListCount('remove') Do Cl.RemoveMember(ListItem('remove', I));
  PCBServer.PostProcess;
  Put('created', JBool(IsNew));
  Put('after', ClassJson(Cl, True));
  If Not PBool('no_save') Then SaveDoc(P('pcb'));
End;

{ pcb_select: lists "d" (designators) and "net"; clear=1 deselects everything first. }
Procedure OpPcbSelect;
Var Board : IPCB_Board; It : IPCB_BoardIterator; O : IPCB_Primitive; I, N : Integer; Want : TStringList;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  If PBool('clear') Then Board.SelectedObjects_Clear;
  Want := TStringList.Create;
  For I := 1 To ListCount('d') Do Want.Add('C:' + UpperCase(ListItem('d', I)));
  For I := 1 To ListCount('net') Do Want.Add('N:' + UpperCase(ListItem('net', I)));
  N := 0;
  It := NewIterator(Board, eComponentObject);
  O := It.FirstPCBObject;
  While O <> Nil Do
  Begin
    If Want.IndexOf('C:' + UpperCase(O.Name.Text)) >= 0 Then
    Begin
      O.Selected := True;
      N := N + 1;
    End;
    O := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  If ListCount('net') > 0 Then
  Begin
    It := Board.BoardIterator_Create;
    It.AddFilter_ObjectSet(MkSet(eTrackObject, eArcObject, eViaObject, ePadObject, ePolyObject, eRegionObject, eFillObject));
    It.AddFilter_LayerSet(AllLayers);
    It.AddFilter_Method(eProcessAll);
    O := It.FirstPCBObject;
    While O <> Nil Do
    Begin
      If O.InNet Then
        If Want.IndexOf('N:' + UpperCase(O.Net.Name)) >= 0 Then
        Begin
          O.Selected := True;
          N := N + 1;
        End;
      O := It.NextPCBObject;
    End;
    Board.BoardIterator_Destroy(It);
  End;
  Want.Free;
  Board.ViewManager_FullUpdate;
  If PBool('zoom') And (N > 0) Then RunWithParams('PCB:Zoom', 'Action=Selected');
  Put('selected', JInt(N));
  Put('selected_total', JInt(Board.SelectedObjectsCount));
End;

{ Board statistics and live per-net data the file parser cannot compute (routed length). }
Procedure OpPcbInfo;
Var Board : IPCB_Board; It : IPCB_BoardIterator; Net : IPCB_Net; S : String; Unrouted : Integer;
Begin
  Board := NeedBoard;
  If Failed Then Exit;
  Log('counting objects');
  Put('pcb', JStr(Board.FileName));
  Put('components', JInt(CountObjects(Board, eComponentObject)));
  Put('nets', JInt(CountObjects(Board, eNetObject)));
  Put('tracks', JInt(CountObjects(Board, eTrackObject)));
  Put('arcs', JInt(CountObjects(Board, eArcObject)));
  Put('vias', JInt(CountObjects(Board, eViaObject)));
  Put('pads', JInt(CountObjects(Board, ePadObject)));
  Put('polygons', JInt(CountObjects(Board, ePolyObject)));
  Put('regions', JInt(CountObjects(Board, eRegionObject)));
  Put('fills', JInt(CountObjects(Board, eFillObject)));
  Put('texts', JInt(CountObjects(Board, eTextObject)));
  Put('rules', JInt(CountObjects(Board, eRuleObject)));
  Put('classes', JInt(CountObjects(Board, eClassObject)));
  Unrouted := CountObjects(Board, eConnectionObject);
  Put('unrouted_connections', JInt(Unrouted));
  Put('board_outline', RectJson(Board.BoardOutline.BoundingRectangle));
  Put('origin', '{"x":' + JC(Board.XOrigin) + ',"y":' + JC(Board.YOrigin) + '}');
  Log('net lengths');
  S := '';
  It := NewIterator(Board, eNetObject);
  Net := It.FirstPCBObject;
  While Net <> Nil Do
  Begin
    S := Join(S, '{"name":' + JStr(Net.Name) + ',"routed_length":' + JC(Net.RoutedLength) +
                 ',"vias":' + JInt(Net.ViaCount) + ',"pins":' + JInt(Net.PinCount) + '}');
    Net := It.NextPCBObject;
  End;
  Board.BoardIterator_Destroy(It);
  Put('net_lengths', '[' + S + ']');
End;

{ ---------- schematic ---------- }

Function SchComponentJson(C : ISch_Component) : String;
Var S, Prm : String; It : ISch_Iterator; Pa : ISch_Parameter;
Begin
  S := Member('designator', JStr(C.Designator.Text));
  S := Join(S, Member('comment', JStr(C.Comment.Text)));
  S := Join(S, Member('lib_ref', JStr(C.LibReference)));
  S := Join(S, Member('x', JC(C.Location.X)));
  S := Join(S, Member('y', JC(C.Location.Y)));
  S := Join(S, Member('orientation', JInt(C.Orientation * 90)));
  Prm := '';
  It := C.SchIterator_Create;
  It.AddFilter_ObjectSet(MkSet(eParameter));
  Pa := It.FirstSchObject;
  While Pa <> Nil Do
  Begin
    Prm := Join(Prm, JStr(Pa.Name) + ':' + JStr(Pa.Text));
    Pa := It.NextSchObject;
  End;
  C.SchIterator_Destroy(It);
  S := Join(S, Member('parameters', '{' + Prm + '}'));
  Result := '{' + S + '}';
End;

Function FindSchComponent(Doc : ISch_Document; Designator : String) : ISch_Component;
Var It : ISch_Iterator; C : ISch_Component;
Begin
  Result := Nil;
  It := Doc.SchIterator_Create;
  It.AddFilter_ObjectSet(MkSet(eSchComponent));
  C := It.FirstSchObject;
  While C <> Nil Do
  Begin
    If UpperCase(C.Designator.Text) = UpperCase(Designator) Then
    Begin
      Result := C;
      Break;
    End;
    C := It.NextSchObject;
  End;
  Doc.SchIterator_Destroy(It);
End;

Procedure OpSchComponents;
Var Doc : ISch_Document; It : ISch_Iterator; C : ISch_Component; S : String;
Begin
  Doc := NeedSheet;
  If Failed Then Exit;
  S := '';
  It := Doc.SchIterator_Create;
  It.AddFilter_ObjectSet(MkSet(eSchComponent));
  C := It.FirstSchObject;
  While C <> Nil Do
  Begin
    S := Join(S, SchComponentJson(C));
    C := It.NextSchObject;
  End;
  Doc.SchIterator_Destroy(It);
  Put('sch', JStr(P('sch')));
  Put('components', '[' + S + ']');
End;

Procedure RegisterSchObject(Doc : ISch_Document; O : ISch_GraphicalObject);
Begin
  Doc.RegisterSchObjectInContainer(O);
  SchServer.RobotManager.SendMessage(Doc.I_ObjectAddress, c_BroadCast, SCHM_PrimitiveRegistration, O.I_ObjectAddress);
End;

{ sch_param: designator, name, value; delete=1 removes the parameter. }
Procedure OpSchParam;
Var Doc : ISch_Document; C : ISch_Component; It : ISch_Iterator; Pa, Found : ISch_Parameter;
    Before, Dry : Boolean;
Begin
  Doc := NeedSheet;
  If Failed Then Exit;
  Dry := PBool('dry_run');
  C := FindSchComponent(Doc, P('designator'));
  If C = Nil Then
  Begin
    Fail('no component ' + P('designator') + ' on ' + P('sch'));
    Exit;
  End;
  Found := Nil;
  It := C.SchIterator_Create;
  It.AddFilter_ObjectSet(MkSet(eParameter));
  Pa := It.FirstSchObject;
  While Pa <> Nil Do
  Begin
    If UpperCase(Pa.Name) = UpperCase(P('name')) Then Found := Pa;
    Pa := It.NextSchObject;
  End;
  C.SchIterator_Destroy(It);
  If Found <> Nil Then Put('before', JStr(Found.Text)) Else Put('before', 'null');
  Put('dry_run', JBool(Dry));
  If Dry Then Exit;
  SchServer.ProcessControl.PreProcess(Doc, '');
  If PBool('delete') Then
  Begin
    If Found <> Nil Then C.RemoveSchObject(Found);
    Put('after', 'null');
  End
  Else If Found <> Nil Then
  Begin
    SchServer.RobotManager.SendMessage(Found.I_ObjectAddress, c_BroadCast, SCHM_BeginModify, c_NoEventData);
    Found.Text := P('value');
    SchServer.RobotManager.SendMessage(Found.I_ObjectAddress, c_BroadCast, SCHM_EndModify, c_NoEventData);
    Put('after', JStr(Found.Text));
  End
  Else
  Begin
    Pa := SchServer.SchObjectFactory(eParameter, eCreate_Default);
    Pa.Name := P('name');
    Pa.Text := P('value');
    Pa.IsHidden := Not PBool('visible');
    Pa.Location := C.Location;
    C.AddSchObject(Pa);
    SchServer.RobotManager.SendMessage(C.I_ObjectAddress, c_BroadCast, SCHM_PrimitiveRegistration, Pa.I_ObjectAddress);
    Put('after', JStr(Pa.Text));
  End;
  SchServer.ProcessControl.PostProcess(Doc, '');
  Doc.GraphicallyInvalidate;
  If Not PBool('no_save') Then SaveDoc(P('sch'));
  Put('component', SchComponentJson(C));
End;

{ sch_place: lib (library path), lib_ref, x, y (internal units), rot (0/90/180/270), designator. }
Procedure OpSchPlace;
Var Doc : ISch_Document; C : ISch_Component; Params : String; Rot : Integer;
Begin
  Doc := NeedSheet;
  If Failed Then Exit;
  Rot := (PInt('rot', 0) Div 90) Mod 4;
  If PBool('dry_run') Then
  Begin
    Put('dry_run', 'true');
    Exit;
  End;
  Params := 'Orientation=' + IntToStr(Rot) + '|Location.X=' + P('x') + '|Location.Y=' + P('y');
  If P('designator') <> '' Then Params := Params + '|Designator=' + P('designator');
  Log('placing ' + P('lib_ref') + ' from ' + P('lib'));
  IntegratedLibraryManager.PlaceLibraryComponent(P('lib_ref'), P('lib'), 'ModelType=SYM|' + Params);
  Doc.GraphicallyInvalidate;
  If P('designator') <> '' Then
  Begin
    C := FindSchComponent(Doc, P('designator'));
    If C <> Nil Then Put('component', SchComponentJson(C));
  End;
  If Not PBool('no_save') Then SaveDoc(P('sch'));
  Put('dry_run', 'false');
End;

{ sch_net_label / sch_power_port: text, x, y; style for power (bar|arrow|wave|circle|ground|...) }
Procedure OpSchLabel;
Var Doc : ISch_Document; O : ISch_GraphicalObject; Kind, Style : String;
Begin
  Doc := NeedSheet;
  If Failed Then Exit;
  If PBool('dry_run') Then
  Begin
    Put('dry_run', 'true');
    Exit;
  End;
  Kind := P('what');
  SchServer.ProcessControl.PreProcess(Doc, '');
  If Kind = 'power' Then
  Begin
    O := SchServer.SchObjectFactory(ePowerObject, eCreate_GlobalCopy);
    Style := LowerCase(P('style'));
    If Style = 'ground' Then O.Style := ePowerGndPower
    Else If Style = 'signal_ground' Then O.Style := ePowerGndSignal
    Else If Style = 'earth' Then O.Style := ePowerGndEarth
    Else If Style = 'arrow' Then O.Style := ePowerArrow
    Else If Style = 'wave' Then O.Style := ePowerWave
    Else If Style = 'circle' Then O.Style := ePowerCircle
    Else O.Style := ePowerBar;
    O.ShowNetName := True;
  End
  Else
    O := SchServer.SchObjectFactory(eNetlabel, eCreate_GlobalCopy);
  O.Location := Point(PInt('x', 0), PInt('y', 0));
  O.Text := P('text');
  O.Orientation := (PInt('rot', 0) Div 90) Mod 4;
  RegisterSchObject(Doc, O);
  SchServer.ProcessControl.PostProcess(Doc, '');
  Doc.GraphicallyInvalidate;
  If Not PBool('no_save') Then SaveDoc(P('sch'));
  Put('placed', JStr(Kind));
  Put('text', JStr(O.Text));
  Put('dry_run', 'false');
End;

{ sch_wire: list "x" and "y" of vertices (internal units), at least two. }
Procedure OpSchWire;
Var Doc : ISch_Document; W : ISch_Wire; I, N : Integer;
Begin
  Doc := NeedSheet;
  If Failed Then Exit;
  N := ListCount('x');
  If N < 2 Then
  Begin
    Fail('a wire needs at least two points');
    Exit;
  End;
  If PBool('dry_run') Then
  Begin
    Put('dry_run', 'true');
    Exit;
  End;
  SchServer.ProcessControl.PreProcess(Doc, '');
  W := SchServer.SchObjectFactory(eWire, eCreate_GlobalCopy);
  W.Location := Point(StrToIntDef(ListItem('x', 1), 0), StrToIntDef(ListItem('y', 1), 0));
  For I := 1 To N Do
  Begin
    W.InsertVertex := I;
    W.SetState_Vertex(I, Point(StrToIntDef(ListItem('x', I), 0), StrToIntDef(ListItem('y', I), 0)));
  End;
  RegisterSchObject(Doc, W);
  SchServer.ProcessControl.PostProcess(Doc, '');
  Doc.GraphicallyInvalidate;
  If Not PBool('no_save') Then SaveDoc(P('sch'));
  Put('vertices', JInt(N));
  Put('dry_run', 'false');
End;

Procedure OpSave;
Begin
  SaveDoc(P('path'));
  Put('saved', JStr(P('path')));
End;

Procedure OpClose;
Var SD : IServerDocument;
Begin
  SD := Client.GetDocumentByPath(P('path'));
  If SD <> Nil Then Client.CloseDocument(SD);
  Put('closed', JBool(SD <> Nil));
End;

{ ---------- dispatch ---------- }

Procedure DispatchOp(Op : String);
Begin
  If Op = 'ping' Then OpPing
  Else If Op = 'documents' Then OpDocuments
  Else If Op = 'open' Then OpOpen
  Else If Op = 'close' Then OpClose
  Else If Op = 'save' Then OpSave
  Else If Op = 'compile' Then OpCompile
  Else If Op = 'netlist' Then OpNetlist
  Else If Op = 'variants' Then OpVariants
  Else If Op = 'annotate' Then OpAnnotate
  Else If Op = 'drc' Then OpDrc
  Else If Op = 'outjob' Then OpOutjob
  Else If Op = 'process' Then OpProcess
  Else If Op = 'save_all' Then OpSaveAll
  Else If Op = 'close_all' Then OpCloseAll
  Else If Op = 'pcb_components' Then OpPcbComponents
  Else If Op = 'pcb_edit' Then OpPcbEdit
  Else If Op = 'pcb_polygons' Then OpPcbPolygons
  Else If Op = 'pcb_rules' Then OpPcbRules
  Else If Op = 'pcb_rule' Then OpPcbRule
  Else If Op = 'pcb_classes' Then OpPcbClasses
  Else If Op = 'pcb_select' Then OpPcbSelect
  Else If Op = 'pcb_info' Then OpPcbInfo
  Else If Op = 'sch_components' Then OpSchComponents
  Else If Op = 'sch_param' Then OpSchParam
  Else If Op = 'sch_place' Then OpSchPlace
  Else If Op = 'sch_label' Then OpSchLabel
  Else If Op = 'sch_wire' Then OpSchWire
  Else
  Begin
    Fail('unknown op: ' + Op);
    Exit;
  End;
  If Not FileExists(JobDir + 'done') Then Finish(True, '');
End;

Procedure Run;
Var Cur : TStringList; Root, Id : String;
Begin
  Root := 'C:\altium-cli-extended\jobs\';
  Cur := TStringList.Create;
  Cur.LoadFromFile(Root + 'current.txt');
  Id := Trim(Cur.Strings[0]);
  Cur.Free;
  JobDir := Root + Id + '\';
  Data := '';
  Stage := 'start';
  Failed := False;
  LogBuf := TStringList.Create;
  Req := TStringList.Create;
  Req.LoadFromFile(JobDir + 'request.ini');
  Log('op ' + P('op'));
  Try
    DispatchOp(P('op'));
  Except
    If Not FileExists(JobDir + 'done') Then Finish(False, 'Altium error during "' + Stage + '": ' + ExceptionMessage);
  End;
  Req.Free;
  LogBuf.Free;
End;
