import * as vscode from "vscode";
import { BlameService, BlameResult, LineBlameInfo } from "./blame-service";

export class BlameLensManager implements vscode.CodeLensProvider {
  private context: vscode.ExtensionContext;
  private blameService: BlameService;
  private currentBlameResult: BlameResult | null = null;
  private currentDocumentUri: string | null = null;
  private pendingBlameRequest: Promise<BlameResult | null> | null = null;
  private statusBarItem: vscode.StatusBarItem;
  private currentSelection: vscode.Selection | null = null;
  private _onDidChangeCodeLenses: vscode.EventEmitter<void> = new vscode.EventEmitter<void>();
  public readonly onDidChangeCodeLenses: vscode.Event<void> = this._onDidChangeCodeLenses.event;

  constructor(context: vscode.ExtensionContext) {
    this.context = context;
    this.blameService = new BlameService();

    // Create status bar item for model display
    this.statusBarItem = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      500
    );
    this.statusBarItem.name = 'git-ai Model';
    this.statusBarItem.hide();
  }

  public activate(): void {
    // Register CodeLens provider for all languages
    this.context.subscriptions.push(
      vscode.languages.registerCodeLensProvider({ scheme: '*', language: '*' }, this)
    );

    // Register selection change listener
    this.context.subscriptions.push(
      vscode.window.onDidChangeTextEditorSelection((event) => {
        this.handleSelectionChange(event);
      })
    );

    // Register hover provider for all languages
    this.context.subscriptions.push(
      vscode.languages.registerHoverProvider({ scheme: '*', language: '*' }, {
        provideHover: (document, position, token) => {
          return this.provideHover(document, position, token);
        }
      })
    );

    // Handle tab/document close to cancel pending blames
    this.context.subscriptions.push(
      vscode.workspace.onDidCloseTextDocument((document) => {
        this.handleDocumentClose(document);
      })
    );

    // Handle active editor change to refresh CodeLens when switching documents
    this.context.subscriptions.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        this.handleActiveEditorChange(editor);
      })
    );

    // Handle file save to invalidate cache and potentially refresh blame
    this.context.subscriptions.push(
      vscode.workspace.onDidSaveTextDocument((document) => {
        this.handleDocumentSave(document);
      })
    );

    // Register CodeLens click handler
    this.context.subscriptions.push(
      vscode.commands.registerCommand('git-ai.showAuthorDetails', (lineInfo: LineBlameInfo, line: number) => {
        this.handleCodeLensClick(lineInfo);
      })
    );

    // Register status bar item click handler
    this.statusBarItem.command = 'git-ai.showModelHover';
    this.context.subscriptions.push(
      vscode.commands.registerCommand('git-ai.showModelHover', () => {
        this.handleStatusBarClick();
      })
    );

    // Add status bar item to context subscriptions for proper cleanup
    this.context.subscriptions.push(this.statusBarItem);

    console.log('[git-ai] BlameLensManager activated, CodeLens provider registered');
  }

  /**
   * Handle document save - invalidate cache and refresh blame if there's an active selection.
   */
  private handleDocumentSave(document: vscode.TextDocument): void {
    const documentUri = document.uri.toString();
    
    // Invalidate cached blame for this document
    this.blameService.invalidateCache(document.uri);
    
    // If this is the current document with blame, clear and re-fetch
    if (this.currentDocumentUri === documentUri) {
      this.currentBlameResult = null;
      this.pendingBlameRequest = null;
      
      // Check if there's a multi-line selection in the active editor
      const activeEditor = vscode.window.activeTextEditor;
      if (activeEditor && activeEditor.document.uri.toString() === documentUri) {
        const selection = activeEditor.selections[0];
        if (selection && selection.start.line !== selection.end.line) {
          // Re-fetch blame with the current selection
          this.requestBlameAndRefresh(activeEditor, selection);
        }
      }
    }
    
    console.log('[git-ai] Document saved, invalidated blame cache for:', document.uri.fsPath);
  }

  /**
   * Handle document close - cancel any pending blame requests and clean up cache.
   */
  private handleDocumentClose(document: vscode.TextDocument): void {
    const documentUri = document.uri.toString();
    
    // Cancel any pending blame for this document
    this.blameService.cancelForUri(document.uri);
    
    // Clear cached blame result if it matches
    if (this.currentDocumentUri === documentUri) {
      this.currentBlameResult = null;
      this.currentDocumentUri = null;
      this.pendingBlameRequest = null;
    }
    
    // Invalidate cache
    this.blameService.invalidateCache(document.uri);
    
    console.log('[git-ai] Document closed, cancelled blame for:', document.uri.fsPath);
  }

  /**
   * Handle active editor change - refresh CodeLens and reset state.
   */
  private handleActiveEditorChange(editor: vscode.TextEditor | undefined): void {
    // Reset selection state
    this.currentSelection = null;
    this.statusBarItem.hide();
    
    // If the new editor is a different document, reset our state
    if (editor && editor.document.uri.toString() !== this.currentDocumentUri) {
      this.currentBlameResult = null;
      this.currentDocumentUri = null;
      this.pendingBlameRequest = null;
    }
    
    // Refresh CodeLens for the new editor
    this._onDidChangeCodeLenses.fire();
  }

  private handleSelectionChange(event: vscode.TextEditorSelectionChangeEvent): void {
    const editor = event.textEditor;
    const selection = event.selections[0]; // Primary selection

    console.log('[git-ai] Selection changed:', {
      hasSelection: !!selection,
      hasEditor: !!editor,
      isMultiLine: selection ? selection.start.line !== selection.end.line : false
    });

    if (!selection || !editor) {
      this.currentSelection = null;
      this.updateStatusBarForCurrentLine(editor);
      this._onDidChangeCodeLenses.fire();
      return;
    }

    // Check if multiple lines are selected
    const isMultiLine = selection.start.line !== selection.end.line;

    if (isMultiLine) {
      console.log('[git-ai] Multi-line selection detected, requesting blame');
      this.currentSelection = selection;
      // Request blame for this document and refresh CodeLens
      this.requestBlameAndRefresh(editor, selection);
    } else {
      // Single line - update status bar based on current line
      console.log('[git-ai] Single line selection, updating status bar for line');
      this.currentSelection = null;
      this.updateStatusBarForCurrentLine(editor);
      this._onDidChangeCodeLenses.fire();
    }
  }

  /**
   * Update status bar based on the current line (cursor position).
   * Shows model name if the current line is AI-authored, otherwise shows human emoji.
   */
  private async updateStatusBarForCurrentLine(editor: vscode.TextEditor | undefined): Promise<void> {
    if (!editor) {
      this.statusBarItem.text = '🧑‍💻';
      this.statusBarItem.tooltip = 'Human-authored code';
      this.statusBarItem.show();
      return;
    }

    const document = editor.document;
    const documentUri = document.uri.toString();
    const currentLine = editor.selection.active.line;
    const gitLine = currentLine + 1; // Convert to 1-indexed

    // Check if we have blame for this document
    if (this.currentDocumentUri !== documentUri || !this.currentBlameResult) {
      // Show human emoji while loading
      this.statusBarItem.text = '🧑‍💻';
      this.statusBarItem.tooltip = 'Loading...';
      this.statusBarItem.show();
      
      // Request blame for the document
      try {
        const result = await this.blameService.requestBlame(document, 'normal');
        if (result) {
          this.currentBlameResult = result;
          this.currentDocumentUri = documentUri;
        } else {
          // Keep showing human emoji if blame fails
          this.statusBarItem.text = '🧑‍💻';
          this.statusBarItem.tooltip = 'Human-authored code';
          this.statusBarItem.show();
          return;
        }
      } catch (error) {
        console.error('[git-ai] Failed to get blame for status bar:', error);
        // Keep showing human emoji on error
        this.statusBarItem.text = '🧑‍💻';
        this.statusBarItem.tooltip = 'Human-authored code';
        this.statusBarItem.show();
        return;
      }
    }

    // Check the current line
    const lineInfo = this.currentBlameResult.lineAuthors.get(gitLine);
    if (lineInfo?.isAiAuthored) {
      const model = lineInfo.promptRecord?.agent_id?.model;
      const modelName = this.extractModelName(model);
      if (modelName) {
        const logo = this.getModelLogo(modelName);
        this.statusBarItem.text = logo;
        this.statusBarItem.tooltip = `AI Model: ${modelName}`;
        this.statusBarItem.show();
        console.log('[git-ai] Status bar updated for line', currentLine, 'with model:', modelName, 'logo:', logo);
      } else {
        // Show robot emoji if AI-authored but no model name
        this.statusBarItem.text = '🤖';
        this.statusBarItem.tooltip = 'AI-authored code';
        this.statusBarItem.show();
      }
    } else {
      // Show human emoji for human-authored code
      this.statusBarItem.text = '🧑‍💻';
      this.statusBarItem.tooltip = 'Human-authored code';
      this.statusBarItem.show();
    }
  }

  private async requestBlameAndRefresh(
    editor: vscode.TextEditor,
    selection: vscode.Selection
  ): Promise<void> {
    const document = editor.document;
    const documentUri = document.uri.toString();

    // Check if we already have blame for this document
    if (this.currentDocumentUri === documentUri && this.currentBlameResult) {
      this._onDidChangeCodeLenses.fire();
      this.updateStatusBarForSelection(selection, this.currentBlameResult);
      return;
    }

    // Request blame with high priority (current selection)
    try {
      // Cancel any pending request for a different document
      if (this.currentDocumentUri !== documentUri) {
        this.pendingBlameRequest = null;
      }

      // Start new request if not already pending
      if (!this.pendingBlameRequest) {
        this.pendingBlameRequest = this.blameService.requestBlame(document, 'high');
        this.currentDocumentUri = documentUri;
      }

      const result = await this.pendingBlameRequest;
      this.pendingBlameRequest = null;

      if (result) {
        this.currentBlameResult = result;
        
        // Check if the selection is still valid and editor is still active
        const currentEditor = vscode.window.activeTextEditor;
        if (currentEditor && currentEditor.document.uri.toString() === documentUri) {
          const currentSelection = currentEditor.selections[0];
          if (currentSelection && currentSelection.start.line !== currentSelection.end.line) {
            this._onDidChangeCodeLenses.fire();
            this.updateStatusBarForSelection(currentSelection, result);
          }
        }
      }
    } catch (error) {
      console.error('[git-ai] Blame request failed:', error);
      this.pendingBlameRequest = null;
    }
  }

  /**
   * Provide CodeLens for the document.
   */
  public provideCodeLenses(
    document: vscode.TextDocument,
    token: vscode.CancellationToken
  ): vscode.CodeLens[] | Thenable<vscode.CodeLens[]> {
    const codeLenses: vscode.CodeLens[] = [];

    // Only show CodeLens if there's a multi-line selection
    if (!this.currentSelection || this.currentSelection.start.line === this.currentSelection.end.line) {
      return codeLenses;
    }

    // Only show CodeLens for the current document
    if (document.uri.toString() !== this.currentDocumentUri) {
      return codeLenses;
    }

    // If we don't have blame yet, return empty (we'll refresh when we get it)
    if (!this.currentBlameResult) {
      return codeLenses;
    }

    const startLine = Math.min(this.currentSelection.start.line, this.currentSelection.end.line);
    const endLine = Math.max(this.currentSelection.start.line, this.currentSelection.end.line);

    // Identify AI hunks within the selection
    const aiHunks = this.identifyAiHunks(this.currentBlameResult, startLine, endLine);

    // Create CodeLens for each AI hunk
    for (const hunk of aiHunks) {
      const lineInfo = this.currentBlameResult.lineAuthors.get(hunk.startLine + 1); // Convert to 1-indexed
      
      if (lineInfo?.isAiAuthored) {
        const tool = lineInfo.author;
        const model = lineInfo.promptRecord?.agent_id?.model || 'unknown';
        const humanAuthor = lineInfo.promptRecord?.human_author || '';
        const humanName = this.extractHumanName(humanAuthor);
        const lineCount = hunk.endLine - hunk.startLine + 1;
        const countSuffix = lineCount > 1 ? ` +${lineCount}` : '';
        
        const title = `🤖 ${tool}|${model} <${humanName}>${countSuffix}`;
        
        const range = new vscode.Range(hunk.startLine, 0, hunk.startLine, 0);
        const codeLens = new vscode.CodeLens(range, {
          title: title,
          command: 'git-ai.showAuthorDetails',
          arguments: [lineInfo, hunk.startLine]
        });
        
        codeLenses.push(codeLens);
      }
    }

    return codeLenses;
  }

  /**
   * Resolve CodeLens (optional, but we can use it to lazy-load data if needed).
   */
  public resolveCodeLens(
    codeLens: vscode.CodeLens,
    token: vscode.CancellationToken
  ): vscode.CodeLens | Thenable<vscode.CodeLens> {
    // We've already provided all the info in provideCodeLenses
    return codeLens;
  }

  /**
   * Update status bar based on the current selection.
   */
  private updateStatusBarForSelection(
    selection: vscode.Selection,
    blameResult: BlameResult
  ): void {
    const startLine = Math.min(selection.start.line, selection.end.line);
    const endLine = Math.max(selection.start.line, selection.end.line);

    // Collect unique model names from AI-authored lines
    const modelNames = new Set<string>();
    let aiLineCount = 0;
    for (let line = startLine; line <= endLine; line++) {
      const gitLine = line + 1; // Convert to 1-indexed
      const lineInfo = blameResult.lineAuthors.get(gitLine);
      if (lineInfo?.isAiAuthored) {
        aiLineCount++;
        const model = lineInfo.promptRecord?.agent_id?.model;
        console.log('[git-ai] Found AI line', line, 'with model:', model);
        const modelName = this.extractModelName(model);
        if (modelName) {
          modelNames.add(modelName);
          console.log('[git-ai] Extracted model name:', modelName);
        } else {
          console.log('[git-ai] Failed to extract model name from:', model);
        }
      }
    }

    console.log('[git-ai] Total AI lines in selection:', aiLineCount, 'Unique models:', Array.from(modelNames));

    // Update status bar with model logos
    if (modelNames.size > 0) {
      // Get unique logos for each model
      const logos = Array.from(modelNames).map(name => this.getModelLogo(name));
      const uniqueLogos = Array.from(new Set(logos));
      const logoText = uniqueLogos.join(' ');
      const modelText = Array.from(modelNames).join(' | ');
      this.statusBarItem.text = logoText;
      this.statusBarItem.tooltip = `AI Models: ${modelText}`;
      this.statusBarItem.show();
      console.log('[git-ai] Status bar updated with models:', modelText, 'logos:', logoText);
    } else {
      // Show human emoji if no AI content in selection
      this.statusBarItem.text = '🧑‍💻';
      this.statusBarItem.tooltip = 'Human-authored code';
      this.statusBarItem.show();
      console.log('[git-ai] No AI models found in selection, showing human emoji. AI line count:', aiLineCount);
    }
  }

  /**
   * Get the display text for an author.
   * Returns "🤖 {tool}|{model} <Name (human)>" for AI-authored lines.
   */
  private getAuthorDisplayText(lineInfo: LineBlameInfo | undefined, isLoading: boolean): string {
    if (isLoading) {
      return 'Loading...';
    }

    if (lineInfo?.isAiAuthored) {
      const tool = lineInfo.author;
      const model = lineInfo.promptRecord?.agent_id?.model || 'unknown';
      const humanAuthor = lineInfo.promptRecord?.human_author || '';
      const humanName = this.extractHumanName(humanAuthor);
      
      return `🤖 ${tool}|${model} <${humanName}>`;
    }

    return '';
  }

  /**
   * Extract just the name from a git author string like "Aidan Cunniffe <acunniffe@gmail.com>"
   */
  private extractHumanName(authorString: string): string {
    if (!authorString) {
      return 'Unknown';
    }
    
    // Handle format: "Name <email>"
    const match = authorString.match(/^([^<]+)/);
    if (match) {
      return match[1].trim();
    }
    
    return authorString;
  }

  /**
   * Extract model name from model string (e.g., "claude-3-opus-20240229" -> "Claude")
   * Returns the part before the first "-" with first letter capitalized, or null if no model.
   */
  private extractModelName(modelString: string | undefined): string | null {
    if (!modelString || modelString.trim() === '') {
      return null;
    }
    
    const parts = modelString.split('-');
    const firstPart = parts[0];
    
    if (!firstPart || firstPart.trim() === '') {
      return null;
    }
    
    // Capitalize first letter
    return firstPart.charAt(0).toUpperCase() + firstPart.slice(1);
  }

  /**
   * Get the display icon/logo for a model name.
   * Returns the logo/emoji for the model, or 🤖 as fallback.
   * 
   * To add a new model logo, add an entry to the MODEL_LOGOS map below.
   * You can use:
   * - Unicode emojis: '🤖'
   * - Unicode symbols: '⚡'
   * - Text: 'Claude'
   * - Or any string that will be displayed in the status bar
   */
  private getModelLogo(modelName: string | null): string {
    if (!modelName) {
      return '🤖';
    }

    const MODEL_LOGOS: Record<string, string> = {
      // Claude models
      'Claude': '🤖', // TODO: Replace with Claude logo
      
      // OpenAI/Codex models
      'Openai': '🤖', // TODO: Replace with OpenAI Codex logo
      'Codex': '🤖', // TODO: Replace with OpenAI Codex logo
      'Gpt': '🤖', // TODO: Replace with OpenAI logo
      
      // Cursor
      'Cursor': '🤖', // TODO: Replace with Cursor logo
      
      // Grok
      'Grok': '🤖', // TODO: Replace with Grok logo
      
      // Gemini
      'Gemini': '🤖', // TODO: Replace with Gemini logo
    };

    // Normalize model name for lookup (case-insensitive)
    const normalizedName = modelName.charAt(0).toUpperCase() + modelName.slice(1).toLowerCase();
    
    return MODEL_LOGOS[normalizedName] || MODEL_LOGOS[modelName] || '🤖';
  }

  /**
   * Identify contiguous AI hunks within the selection range.
   * Returns an array of hunks with their start and end lines (0-indexed).
   */
  private identifyAiHunks(
    blameResult: BlameResult,
    startLine: number,
    endLine: number
  ): Array<{ startLine: number; endLine: number; commitHash: string }> {
    const hunks: Array<{ startLine: number; endLine: number; commitHash: string }> = [];
    let currentHunk: { startLine: number; endLine: number; commitHash: string } | null = null;

    for (let line = startLine; line <= endLine; line++) {
      const gitLine = line + 1; // Convert to 1-indexed
      const lineInfo = blameResult.lineAuthors.get(gitLine);
      
      if (lineInfo?.isAiAuthored) {
        const commitHash = lineInfo.commitHash;
        
        if (currentHunk && currentHunk.commitHash === commitHash) {
          // Extend current hunk
          currentHunk.endLine = line;
        } else {
          // Start new hunk (save previous if exists)
          if (currentHunk) {
            hunks.push(currentHunk);
          }
          currentHunk = { startLine: line, endLine: line, commitHash };
        }
      } else {
        // Human-authored line - close current hunk if any
        if (currentHunk) {
          hunks.push(currentHunk);
          currentHunk = null;
        }
      }
    }

    // Don't forget the last hunk
    if (currentHunk) {
      hunks.push(currentHunk);
    }

    return hunks;
  }

  /**
   * Handle CodeLens click - show author details.
   */
  private handleCodeLensClick(lineInfo: LineBlameInfo): void {
    const hoverContent = this.buildHoverContent(lineInfo);
    const mdString = hoverContent.value;
    
    // Show the markdown content as an information message
    vscode.window.showInformationMessage(mdString, { modal: false });
  }

  private provideHover(
    document: vscode.TextDocument,
    position: vscode.Position,
    token: vscode.CancellationToken
  ): vscode.Hover | undefined {
    // Only provide hover if we have a multi-line selection and blame data
    if (!this.currentSelection || !this.currentBlameResult) {
      return undefined;
    }

    // Check if the hover position is within the current selection
    const startLine = Math.min(this.currentSelection.start.line, this.currentSelection.end.line);
    const endLine = Math.max(this.currentSelection.start.line, this.currentSelection.end.line);
    
    if (position.line < startLine || position.line > endLine) {
      return undefined;
    }

    // Get blame info for this line (1-indexed)
    const gitLine = position.line + 1;
    const lineInfo = this.currentBlameResult.lineAuthors.get(gitLine);
    
    // Only show hover for AI-authored lines
    if (lineInfo?.isAiAuthored) {
      const hoverContent = this.buildHoverContent(lineInfo);
      return new vscode.Hover(hoverContent);
    }

    return undefined;
  }

  /**
   * Build hover content showing author details.
   */
  private buildHoverContent(lineInfo: LineBlameInfo | undefined): vscode.MarkdownString {
    const md = new vscode.MarkdownString();
    md.isTrusted = true;

    if (!lineInfo || !lineInfo.isAiAuthored) {
      md.appendMarkdown('**Author:** Human\n\n');
      md.appendText('This line was written by a human.');
      return md;
    }

    const record = lineInfo.promptRecord;
    const model = record?.agent_id?.model || lineInfo.author;
    const tool = lineInfo.author.charAt(0).toUpperCase() + lineInfo.author.slice(1);
    
    md.appendMarkdown(`🤖 **${model}**\n\n`);
    md.appendMarkdown(`**Tool:** ${tool}\n\n`);
    
    if (record?.human_author) {
      md.appendMarkdown(`**Paired with:** ${record.human_author}\n\n`);
    }
    
    // Show the first user message as context
    const userMessage = record?.messages?.find(m => m.type === 'user');
    if (userMessage?.text) {
      const truncatedText = userMessage.text.length > 200 
        ? userMessage.text.substring(0, 200) + '...' 
        : userMessage.text;
      md.appendMarkdown('**Prompt:**\n');
      md.appendCodeblock(truncatedText, 'markdown');
    }

    return md;
  }

  /**
   * Handle status bar item click - show hover content for first AI-authored line.
   */
  private handleStatusBarClick(): void {
    const editor = vscode.window.activeTextEditor;
    if (!editor || !this.currentSelection || !this.currentBlameResult) {
      return;
    }

    const startLine = Math.min(this.currentSelection.start.line, this.currentSelection.end.line);
    const endLine = Math.max(this.currentSelection.start.line, this.currentSelection.end.line);

    // Find first AI-authored line in selection
    let firstAiLine: number | undefined = undefined;
    let firstAiLineInfo: LineBlameInfo | undefined = undefined;
    for (let line = startLine; line <= endLine; line++) {
      const gitLine = line + 1; // Convert to 1-indexed
      const lineInfo = this.currentBlameResult.lineAuthors.get(gitLine);
      if (lineInfo?.isAiAuthored) {
        firstAiLine = line;
        firstAiLineInfo = lineInfo;
        break;
      }
    }

    if (!firstAiLineInfo || firstAiLine === undefined) {
      return;
    }

    // Build hover content
    const hoverContent = this.buildHoverContent(firstAiLineInfo);
    
    // Show the hover content using VS Code's markdown rendering
    // We'll create a hover at the first AI line position
    const position = new vscode.Position(firstAiLine, 0);
    
    // Use VS Code's hover provider to show the content
    // Since we can't programmatically trigger a hover, we'll show it as a message
    // with the markdown content formatted
    const mdString = hoverContent.value;
    
    // Show the markdown content - VS Code's showInformationMessage will display
    // the text, though markdown formatting may not be fully rendered
    // For better UX, we could create a webview, but for now this works
    vscode.window.showInformationMessage(mdString, { modal: false });
  }

  public dispose(): void {
    this.blameService.dispose();
    this.statusBarItem.dispose();
    this._onDidChangeCodeLenses.dispose();
  }
}

/**
 * Register the View Author command (stub for future use)
 */
export function registerBlameLensCommands(context: vscode.ExtensionContext): void {
  context.subscriptions.push(
    vscode.commands.registerCommand('git-ai.viewAuthor', (lineNumber: number) => {
      vscode.window.showInformationMessage('Hello World');
    })
  );
}
