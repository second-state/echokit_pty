class WebTerminal {
    constructor() {
        this.terminal = null;
        this.websocket = null;
        this.fitAddon = null;
        this.currentLine = '';
        this.connected = false;
        this.whisperUrl = '';
        this.whisperToken = '';
        this.whisperLanguage = 'auto';
        this.DEFAULT_WHISPER_URL = 'https://whisper.gaia.domains/v1/audio/transcriptions';
        this.sessionUuid = ''; // Session UUID

        // VAD 相关属性
        this.myvad = null;
        this.isVadActive = false;
        this.vadEnabled = false;
        this.pendingInput = ''; // 待输入的内容

        this.init();
    }

    async init() {
        this.setupTerminal();
        // 从 URL 获取 session id
        const urlParams = new URLSearchParams(window.location.search);
        const sessionId = urlParams.get('id');
        if (sessionId) {
            this.sessionUuid = sessionId;
            this.connectWebSocket();
        } else {
            this.terminal.writeln('\r\n\x1b[33mNo session ID provided. Use ?id=<uuid> in URL to connect.\x1b[0m');
        }
        this.setupEventListeners();
        this.setupThemeController();
        this.setupSettingsModal();
        this.loadSettings();
        this.updateConnectionStatus();
        this.initializeVAD();
    }

    setupTerminal() {
        this.terminal = new Terminal({
            cursorBlink: true,
            theme: {
                background: '#1e1e1e',
                foreground: '#ffffff',
                cursor: '#ffffff',
                selection: 'rgba(255, 255, 255, 0.3)',
                black: '#000000',
                red: '#cd3131',
                green: '#0dbc79',
                yellow: '#e5e510',
                blue: '#2472c8',
                magenta: '#bc3fbc',
                cyan: '#11a8cd',
                white: '#e5e5e5',
                brightBlack: '#666666',
                brightRed: '#f14c4c',
                brightGreen: '#23d18b',
                brightYellow: '#f5f543',
                brightBlue: '#3b8eea',
                brightMagenta: '#d670d6',
                brightCyan: '#29b8db',
                brightWhite: '#e5e5e5'
            },
            fontSize: 14,
            fontFamily: '"Fira Code", "Cascadia Code", "Menlo", "Monaco", monospace',
            cols: 80,
            rows: 24
        });

        this.fitAddon = new FitAddon.FitAddon();
        this.terminal.loadAddon(this.fitAddon);

        const terminalElement = document.getElementById('terminal');
        this.terminal.open(terminalElement);

        setTimeout(() => {
            this.fitAddon.fit();
        }, 100);

        this.terminal.onData(data => {
            if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
                this.sendBytesInput(data);
            }
        });

        this.terminal.writeln('Welcome to Web Terminal');
        this.terminal.writeln('Connecting to server...');
    }

    connectWebSocket() {
        // 确保 UUID 存在
        if (!this.sessionUuid) {
            const modal = document.getElementById('no_uuid_modal');
            modal?.showModal();
            return;
        }

        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/${this.sessionUuid}`;

        this.terminal.writeln(`Connecting with session: ${this.sessionUuid}...`);
        this.websocket = new WebSocket(wsUrl);

        // 关闭旧的 WebSocket 连接
        if (this.oldWebSocket) {
            this.oldWebSocket.close();
            this.oldWebSocket = null;
        }

        this.websocket.onopen = () => {
            this.terminal.clear();
            this.terminal.writeln('Connected to terminal server');
            this.connected = true;
            this.updateConnectionStatus();
            console.log('WebSocket connected');

            // 连接成功后创建会话并获取当前状态
            this.sendCreateSession();
            this.getCurrentState();
        };

        this.websocket.onmessage = (event) => {
            this.handleServerMessage(event.data);
        };

        this.websocket.onclose = () => {
            this.terminal.writeln('\r\n\nConnection closed.');
            this.connected = false;
            this.updateConnectionStatus();
            console.log('WebSocket closed');
            this.showReconnectDialog();
        };

        this.websocket.onerror = (error) => {
            this.terminal.writeln('\r\n\nConnection error occurred');
            console.error('WebSocket error:', error);
        };
    }

    reconnect() {
        const modal = document.getElementById('settings_modal');
        modal?.close();

        if (this.websocket) {
            this.oldWebSocket = this.websocket;
        }

        this.terminal.writeln('\r\n\x1b[33mReconnecting...\x1b[0m');
        this.connectWebSocket();
    }

    setupEventListeners() {
        window.addEventListener('resize', () => {
            if (this.fitAddon) {
                setTimeout(() => {
                    this.fitAddon.fit();
                }, 100);
            }
        });

        window.addEventListener('beforeunload', () => {
            if (this.websocket) {
                this.websocket.close();
            }
        });

        document.querySelector('.control-button.close').addEventListener('click', () => {
            if (confirm('Are you sure you want to close the terminal?')) {
                window.close();
            }
        });

        document.querySelector('.control-button.minimize').addEventListener('click', () => {
            const container = document.querySelector('.container');
            container.style.transform = 'scale(0.9)';
            container.style.transition = 'transform 0.2s ease';
            setTimeout(() => {
                container.style.transform = 'scale(1)';
            }, 200);
        });

        document.querySelector('.control-button.maximize').addEventListener('click', () => {
            document.documentElement.requestFullscreen().catch(err => {
                console.log('Fullscreen not supported:', err);
            });
        });

        document.addEventListener('keydown', (e) => {
            if (e.ctrlKey && e.key === 'c') {
                e.preventDefault();
                this.sendKeyboardInterrupt();
            }
        });

        // VAD Button Event Listener
        const vadBtn = document.getElementById('vad-btn');
        vadBtn?.addEventListener('click', () => {
            this.toggleVAD();
        });

        // Clear Pending Input Button
        const clearPendingBtn = document.getElementById('clear-pending');
        clearPendingBtn?.addEventListener('click', () => {
            this.clearPendingInput();
        });

        // Clear Speech Display Button
        const clearSpeechBtn = document.getElementById('clear-speech');
        clearSpeechBtn?.addEventListener('click', () => {
            this.clearSpeechDisplay();
        });
    }

    sendKeyboardInterrupt() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            this.sendBytesInput('\u0003'); // Ctrl+C
        }
    }

    focus() {
        if (this.terminal) {
            this.terminal.focus();
        }
    }

    updateConnectionStatus() {
        const badge = document.querySelector('.badge');

        if (this.connected) {
            badge?.classList.remove('badge-error');
            badge?.classList.add('badge-success');
            if (badge) badge.innerHTML = '<div class="w-2 h-2 rounded-full bg-success animate-pulse"></div>Connected';
        } else {
            badge?.classList.remove('badge-success');
            badge?.classList.add('badge-error');
            if (badge) badge.innerHTML = '<div class="w-2 h-2 rounded-full bg-error"></div>Disconnected';
        }
    }

    setupThemeController() {
        const themeControllers = document.querySelectorAll('.theme-controller');

        themeControllers.forEach(controller => {
            controller.addEventListener('click', (e) => {
                e.preventDefault();
                const theme = controller.getAttribute('data-theme');
                document.documentElement.setAttribute('data-theme', theme);

                // Update terminal theme based on DaisyUI theme
                this.updateTerminalTheme(theme);

                // Store theme preference
                localStorage.setItem('terminal-theme', theme);
            });
        });

        // Load saved theme
        const savedTheme = localStorage.getItem('terminal-theme') || 'dark';
        document.documentElement.setAttribute('data-theme', savedTheme);
        this.updateTerminalTheme(savedTheme);
    }

    updateTerminalTheme(theme) {
        if (!this.terminal) return;

        const themes = {
            'dark': {
                background: '#1f2937',
                foreground: '#f9fafb',
                cursor: '#3b82f6'
            },
            'light': {
                background: '#ffffff',
                foreground: '#111827',
                cursor: '#3b82f6'
            },
            'cyberpunk': {
                background: '#0a0a0a',
                foreground: '#00ff00',
                cursor: '#ff00ff'
            },
            'synthwave': {
                background: '#1a1a2e',
                foreground: '#ff6b9d',
                cursor: '#00d2ff'
            }
        };

        const selectedTheme = themes[theme] || themes.dark;

        this.terminal.options.theme = {
            ...this.terminal.options.theme,
            background: selectedTheme.background,
            foreground: selectedTheme.foreground,
            cursor: selectedTheme.cursor
        };
    }

    showReconnectDialog() {
        const modal = document.getElementById('reconnect_modal');
        const reconnectBtn = document.getElementById('reconnect-btn');

        // Remove existing event listeners to prevent duplicates
        const newReconnectBtn = reconnectBtn.cloneNode(true);
        reconnectBtn.parentNode.replaceChild(newReconnectBtn, reconnectBtn);

        // Add new event listener
        newReconnectBtn.addEventListener('click', () => {
            modal.close();
            this.terminal.writeln('Attempting to reconnect...');
            this.connectWebSocket();
        });

        modal.showModal();
    }

    // 调试方法 - 获取终端缓冲区内容
    getBuffer() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const lines = [];
        for (let i = 0; i < buffer.length; i++) {
            const line = buffer.getLine(i);
            if (line) {
                lines.push(line.translateToString(true));
            }
        }
        return lines;
    }

    // 调试方法 - 获取当前行内容
    getCurrentLine() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const currentLine = buffer.getLine(buffer.cursorY);
        return currentLine ? currentLine.translateToString(true) : null;
    }

    // 调试方法 - 获取指定行内容
    getLine(lineNumber) {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const line = buffer.getLine(lineNumber);
        return line ? line.translateToString(true) : null;
    }

    // 调试方法 - 获取光标位置
    getCursorPosition() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        return {
            x: buffer.cursorX,
            y: buffer.cursorY,
            line: this.getCurrentLine()
        };
    }

    // 调试方法 - 获取终端统计信息
    getTerminalInfo() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        return {
            cols: this.terminal.cols,
            rows: this.terminal.rows,
            bufferLength: buffer.length,
            cursorX: buffer.cursorX,
            cursorY: buffer.cursorY,
            connected: this.connected
        };
    }

    // 调试方法 - 获取可视区域的所有内容
    getVisibleContent() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const lines = [];
        const viewportStart = buffer.viewportY;
        const viewportEnd = Math.min(viewportStart + this.terminal.rows, buffer.length);

        for (let i = viewportStart; i < viewportEnd; i++) {
            const line = buffer.getLine(i);
            lines.push({
                index: i,
                content: line ? line.translateToString(true) : '',
                isCursorLine: i === buffer.cursorY
            });
        }
        return lines;
    }

    getRecentLines(count = 10) {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const lines = [];
        const start = Math.max(0, buffer.cursorY - count + 1);

        for (let i = start; i <= buffer.cursorY; i++) {
            const line = buffer.getLine(i);
            lines.push({
                index: i,
                content: line ? line.translateToString(true) : '',
                isCursorLine: i === buffer.cursorY
            });
        }
        return lines;
    }

    getNonEmptyLines() {
        if (!this.terminal) return null;
        const buffer = this.terminal.buffer.active;
        const lines = [];

        for (let i = 0; i < buffer.length; i++) {
            const line = buffer.getLine(i);
            if (line) {
                const content = line.translateToString(true).trim();
                if (content) {
                    lines.push({
                        index: i,
                        content: content,
                        isCursorLine: i === buffer.cursorY
                    });
                }
            }
        }
        return lines;
    }

    // 调试方法 - 实时监控终端变化
    startMonitoring(callback) {
        if (!this.terminal) return null;

        const monitor = () => {
            const info = {
                cursorPosition: this.getCursorPosition(),
                currentLine: this.getCurrentLine(),
                recentLines: this.getRecentLines(5),
                timestamp: new Date().toLocaleTimeString()
            };

            if (callback) {
                callback(info);
            } else {
                console.log('Terminal Monitor:', info);
            }
        };

        // 每秒监控一次
        const intervalId = setInterval(monitor, 1000);

        // 返回停止函数
        return () => clearInterval(intervalId);
    }

    // 设置相关方法
    setupSettingsModal() {
        const settingsBtn = document.getElementById('settings-btn');
        const saveBtn = document.getElementById('save-settings-btn');
        const reconnectBtn = document.getElementById('reconnect-settings-btn');
        const testWhisperBtn = document.getElementById('test-whisper-btn');
        const resetWhisperBtn = document.getElementById('reset-whisper-btn');
        const toggleTokenBtn = document.getElementById('toggle-token-visibility');
        const clearTokenBtn = document.getElementById('clear-token-btn');

        // 打开设置模态框
        settingsBtn?.addEventListener('click', () => {
            this.openSettingsModal();
        });

        // 保存设置
        saveBtn?.addEventListener('click', () => {
            this.saveSettings();
        });

        // 重连
        reconnectBtn?.addEventListener('click', () => {
            this.reconnect();
        });

        // 测试 Whisper 连接
        testWhisperBtn?.addEventListener('click', () => {
            this.testWhisperConnection();
        });

        // 重置 Whisper URL 到默认值
        resetWhisperBtn?.addEventListener('click', () => {
            this.resetWhisperUrl();
        });

        // 切换 Token 可见性
        toggleTokenBtn?.addEventListener('click', () => {
            this.toggleTokenVisibility();
        });

        // 清除 Token
        clearTokenBtn?.addEventListener('click', () => {
            this.clearToken();
        });
    }

    openSettingsModal() {
        const modal = document.getElementById('settings_modal');
        const whisperUrlInput = document.getElementById('whisper-url-input');
        const whisperTokenInput = document.getElementById('whisper-token-input');
        const whisperLanguageSelect = document.getElementById('whisper-language-select');

        // 加载当前设置到输入框
        if (whisperUrlInput) {
            whisperUrlInput.value = this.whisperUrl || '';
        }
        if (whisperTokenInput) {
            whisperTokenInput.value = this.whisperToken || '';
        }
        if (whisperLanguageSelect) {
            whisperLanguageSelect.value = this.whisperLanguage || 'auto';
        }

        modal?.showModal();
    }

    saveSettings() {
        const whisperUrlInput = document.getElementById('whisper-url-input');
        const whisperTokenInput = document.getElementById('whisper-token-input');
        const whisperLanguageSelect = document.getElementById('whisper-language-select');
        const modal = document.getElementById('settings_modal');

        if (whisperUrlInput && whisperTokenInput && whisperLanguageSelect) {
            const newWhisperUrl = whisperUrlInput.value.trim();
            const newWhisperToken = whisperTokenInput.value.trim();
            const newWhisperLanguage = whisperLanguageSelect.value;

            // 验证 URL 格式
            if (newWhisperUrl && !this.isValidUrl(newWhisperUrl)) {
                this.showToast('Invalid URL format', 'error');
                return;
            }

            this.whisperUrl = newWhisperUrl;
            this.whisperToken = newWhisperToken;
            this.whisperLanguage = newWhisperLanguage;

            // 保存 URL 到 localStorage，Token 和语言不保存
            localStorage.setItem('whisper-url', this.whisperUrl);

            this.showToast('Settings saved successfully', 'success');
            this.updateWhisperStatus();

            modal?.close();
        }
    }

    loadSettings() {
        // 从 localStorage 加载 URL 设置，Token 和语言不持久化保存
        this.whisperUrl = localStorage.getItem('whisper-url') || this.DEFAULT_WHISPER_URL;
        this.whisperLanguage = 'auto'; // 语言每次启动都重置为 auto
        this.whisperToken = ''; // Token 每次启动都重置为空
        this.updateWhisperStatus();
    }

    async testWhisperConnection() {
        const whisperUrlInput = document.getElementById('whisper-url-input');
        const testBtn = document.getElementById('test-whisper-btn');
        const url = whisperUrlInput?.value.trim();

        if (!url) {
            this.showToast('Please enter a Whisper URL first', 'warning');
            return;
        }

        if (!this.isValidUrl(url)) {
            this.showToast('Invalid URL format', 'error');
            return;
        }

        // 更新按钮状态
        if (testBtn) {
            testBtn.disabled = true;
            testBtn.innerHTML = '<span class="loading loading-spinner loading-xs"></span> Testing...';
        }

        try {
            // 测试连接到 Whisper 服务器
            // 对于 Whisper API，我们发送一个 OPTIONS 请求来检查 CORS 和可用性
            const response = await fetch(url, {
                method: 'OPTIONS',
                timeout: 5000,
                headers: {
                    'Origin': window.location.origin
                }
            });

            if (response.ok || response.status === 405) {
                // 405 Method Not Allowed 也表示服务器是可达的
                this.showToast('Whisper server is reachable!', 'success');
                this.updateWhisperStatus(true);
            } else {
                throw new Error(`HTTP ${response.status}`);
            }
        } catch (error) {
            console.error('Whisper connection test failed:', error);
            this.showToast('Connection failed: ' + error.message, 'error');
            this.updateWhisperStatus(false);
        } finally {
            // 恢复按钮状态
            if (testBtn) {
                testBtn.disabled = false;
                testBtn.innerHTML = 'Test';
            }
        }
    }

    updateWhisperStatus(connected = null) {
        const statusElement = document.getElementById('whisper-status');
        if (!statusElement) return;

        let statusHtml;
        if (connected === true) {
            statusHtml = '<div class="badge badge-success"><div class="w-2 h-2 rounded-full bg-success mr-2"></div>Connected</div>';
        } else if (connected === false) {
            statusHtml = '<div class="badge badge-error"><div class="w-2 h-2 rounded-full bg-error mr-2"></div>Connection Failed</div>';
        } else if (this.whisperUrl) {
            statusHtml = '<div class="badge badge-warning"><div class="w-2 h-2 rounded-full bg-warning mr-2"></div>Not Tested</div>';
        } else {
            statusHtml = '<div class="badge badge-neutral"><div class="w-2 h-2 rounded-full bg-base-content opacity-60 mr-2"></div>Not Configured</div>';
        }

        statusElement.innerHTML = statusHtml;
    }

    isValidUrl(string) {
        try {
            new URL(string);
            return true;
        } catch (_) {
            return false;
        }
    }

    showToast(message, type = 'info') {
        // 创建 toast 通知
        const toast = document.createElement('div');
        toast.className = `alert alert-${type} fixed top-4 right-4 w-auto max-w-sm z-50 shadow-lg`;
        toast.innerHTML = `
            <svg xmlns="http://www.w3.org/2000/svg" class="stroke-current shrink-0 h-6 w-6" fill="none" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <span>${message}</span>
        `;

        document.body.appendChild(toast);

        // 3秒后自动移除
        setTimeout(() => {
            toast.remove();
        }, 3000);
    }

    resetWhisperUrl() {
        const whisperUrlInput = document.getElementById('whisper-url-input');
        if (whisperUrlInput) {
            whisperUrlInput.value = this.DEFAULT_WHISPER_URL;
            this.showToast('Reset to default URL', 'info');
            this.updateWhisperStatus(); // 重置状态为未测试
        }
    }

    toggleTokenVisibility() {
        const tokenInput = document.getElementById('whisper-token-input');
        const eyeIcon = document.getElementById('eye-icon');

        if (tokenInput && eyeIcon) {
            const isPassword = tokenInput.type === 'password';
            tokenInput.type = isPassword ? 'text' : 'password';

            // 更新眼睛图标
            if (isPassword) {
                // 显示状态 - 眼睛斜杠图标
                eyeIcon.innerHTML = `
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.875 18.825A10.05 10.05 0 0112 19c-4.478 0-8.268-2.943-9.543-7a9.97 9.97 0 011.563-3.029m5.858.908a3 3 0 114.243 4.243M9.878 9.878l4.242 4.242M9.878 9.878L3 3m6.878 6.878L21 21" />
                `;
            } else {
                // 隐藏状态 - 正常眼睛图标
                eyeIcon.innerHTML = `
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                `;
            }
        }
    }

    clearToken() {
        const tokenInput = document.getElementById('whisper-token-input');
        if (tokenInput) {
            tokenInput.value = '';
            this.showToast('Token cleared', 'info');
        }
    }

    // 获取 Whisper URL（供其他功能使用）
    getWhisperUrl() {
        return this.whisperUrl;
    }

    // 获取 Whisper Token（供其他功能使用）
    getWhisperToken() {
        return this.whisperToken;
    }

    // 获取完整的 Whisper 配置
    getWhisperConfig() {
        return {
            url: this.whisperUrl,
            token: this.whisperToken,
            language: this.whisperLanguage
        };
    }

    // UUID 相关方法
    isValidUuid(uuid) {
        const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
        return uuidRegex.test(uuid);
    }


    // VAD 相关方法
    async initializeVAD() {
        try {
            console.log('正在初始化 VAD...');

            // 检查是否支持 VAD
            if (!window.vad) {
                console.warn('VAD library not loaded');
                return;
            }

            this.myvad = await vad.MicVAD.new({
                onSpeechStart: () => {
                    this.handleSpeechStart();
                },
                onSpeechEnd: (audio) => {
                    this.handleSpeechEnd(audio);
                },
                onVADMisfire: () => {
                    this.handleVADMisfire();
                },
            });

            console.log('✅ VAD 初始化完成');
            this.vadEnabled = true;
            this.updateVADButton();

        } catch (error) {
            console.error('❌ VAD 初始化失败:', error);
            this.vadEnabled = false;
            this.updateVADButton();
        }
    }

    async toggleVAD() {
        if (!this.vadEnabled || !this.myvad) {
            this.showToast('VAD not available', 'error');
            return;
        }

        if (this.isVadActive) {
            this.myvad.pause();
            this.isVadActive = false;
            console.log('⏹️ VAD 已停止');
        } else {
            try {
                await this.myvad.start();
                this.isVadActive = true;
                console.log('🎧 VAD 开始监听');
            } catch (error) {
                console.error('❌ VAD 启动失败:', error);
                this.showToast('Failed to start VAD: ' + error.message, 'error');
            }
        }

        this.updateVADButton();
        this.updateVADStatus();
    }

    handleSpeechStart() {
        console.log('🎤 检测到语音开始');
        this.updateVADStatus(true);
    }

    handleSpeechEnd(audio) {
        console.log(`🔇 语音结束 - 采样点: ${audio.length}`);
        this.updateVADStatus(false);
        this.processSpeechAudio(audio);
    }

    handleVADMisfire() {
        console.log('⚠️ VAD 误触发');
    }

    async processSpeechAudio(audioData) {
        try {
            // 如果配置了 Whisper URL，进行语音识别
            if (this.whisperUrl) {
                await this.transcribeAudio(audioData);
            } else {
                this.showToast('Whisper URL not configured', 'warning');
            }
        } catch (error) {
            console.error('处理语音音频失败:', error);
            this.showToast('Failed to process speech: ' + error.message, 'error');
        }
    }

    processVoiceCommand(transcription) {
        const text = transcription.trim().toLowerCase();

        // 处理确认指令 - 移除标点符号并处理多种变体
        const cleanText = text.replace(/[.,!?;:"']/g, '').trim().toLowerCase();
        if (cleanText === 'ok' || cleanText === 'okay' || cleanText === 'yes' || cleanText === '确认') {
            if (this.pendingInput) {
                this.sendTextToTerminal(this.pendingInput);
                this.clearPendingInput();
            } else {
                // 如果没有待输入内容，发送 confirm 消息
                this.sendConfirm();
            }
            return true;
        }

        // 处理方向键指令
        if (cleanText === 'up' || cleanText === 'previous' || cleanText === '向上') {
            this.sendArrowKey('up');
            return true;
        }

        if (cleanText === 'down' || cleanText === 'next' || cleanText === '向下') {
            this.sendArrowKey('down');
            return true;
        }

        if (cleanText === 'left' || cleanText === '向左') {
            this.sendArrowKey('left');
            return true;
        }

        if (cleanText === 'right' || cleanText === '向右') {
            this.sendArrowKey('right');
            return true;
        }

        // 处理中断指令
        if (cleanText === 'interrupt' || cleanText === '中断') {
            this.sendKeyboardInterrupt();
            return true;
        }

        return false;
    }

    setPendingInput(content) {
        this.pendingInput = content;
        this.updatePendingInputDisplay();
        console.log('设置待输入内容:', content);
    }

    clearPendingInput() {
        this.pendingInput = '';
        this.updatePendingInputDisplay();
        console.log('清除待输入内容');
    }

    updatePendingInputDisplay() {
        const pendingInputDiv = document.getElementById('pending-input');
        const pendingTextSpan = document.getElementById('pending-text');

        if (!pendingInputDiv || !pendingTextSpan) return;

        if (this.pendingInput) {
            pendingTextSpan.textContent = this.pendingInput;
            pendingInputDiv.classList.remove('hidden');
        } else {
            pendingInputDiv.classList.add('hidden');
        }
    }

    showSpeechDisplay(text) {
        const speechDisplayDiv = document.getElementById('speech-display');
        const speechTextSpan = document.getElementById('speech-text');

        if (!speechDisplayDiv || !speechTextSpan) return;

        speechTextSpan.textContent = text;
        speechDisplayDiv.classList.remove('hidden');
        console.log('显示听到的内容:', text);

        // 3秒后自动隐藏
        setTimeout(() => {
            this.clearSpeechDisplay();
        }, 3000);
    }

    clearSpeechDisplay() {
        const speechDisplayDiv = document.getElementById('speech-display');
        if (speechDisplayDiv) {
            speechDisplayDiv.classList.add('hidden');
        }
        console.log('清除语音显示');
    }

    async transcribeAudio(audioData) {
        try {
            // 创建 WAV 文件
            const wavBlob = this.createWavFile(audioData, 16000);

            // 创建 FormData
            const formData = new FormData();
            formData.append('file', wavBlob, 'audio.wav');
            formData.append('model', 'whisper-1');

            // 添加语言参数（如果不是auto的话）
            if (this.whisperLanguage && this.whisperLanguage !== 'auto') {
                formData.append('language', this.whisperLanguage);
            }

            // 设置请求头
            const headers = {
                'Accept': 'application/json'
            };

            // 如果有 token，添加授权头
            if (this.whisperToken) {
                headers['Authorization'] = `Bearer ${this.whisperToken}`;
            }

            // 发送请求到 Whisper API
            const response = await fetch(this.whisperUrl, {
                method: 'POST',
                headers: headers,
                body: formData
            });

            if (!response.ok) {
                throw new Error(`HTTP ${response.status}: ${response.statusText}`);
            }

            const result = await response.json();
            let transcription = result.text || '';

            // 去掉时间戳，如 [00:00:00.000 --> 00:00:00.960]
            transcription = this.removeTimestamps(transcription);

            if (transcription.trim()) {
                // 首先尝试处理语音指令
                if (!this.processVoiceCommand(transcription.trim())) {
                    // 如果不是指令，放入待输入区
                    this.setPendingInput(transcription.trim());
                }
            }

        } catch (error) {
            console.error('语音转录失败:', error);
            this.showToast('Speech transcription failed: ' + error.message, 'error');
        }
    }

    removeTimestamps(text) {
        // 移除时间戳格式: [.*? --> .*?]
        return text.replace(/\[.*?-->.*?\]/g, '').trim();
    }

    sendTextToTerminal(text) {
        // 发送文本到终端
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            this.sendBytesInput(text);
            console.log('发送文本到终端:', text);
        }
    }

    sendEnterKey() {
        // 发送回车键到终端 - 多种方式
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            this.sendBytesInput('\x0D');
            console.log('发送回车键到终端');
        }
    }

    sendArrowKey(direction) {
        // 发送方向键到终端
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            let arrowSequence = '';
            switch (direction) {
                case 'up':
                    arrowSequence = '\x1b[A'; // ESC[A
                    break;
                case 'down':
                    arrowSequence = '\x1b[B'; // ESC[B
                    break;
                case 'right':
                    arrowSequence = '\x1b[C'; // ESC[C
                    break;
                case 'left':
                    arrowSequence = '\x1b[D'; // ESC[D
                    break;
            }

            if (arrowSequence) {
                this.sendBytesInput(arrowSequence);
                console.log('发送方向键到终端:', direction);
            }
        }
    }

    createWavFile(pcmData, sampleRate) {
        const length = pcmData.length;
        const buffer = new ArrayBuffer(44 + length * 2);
        const view = new DataView(buffer);

        // WAV 头部
        const writeString = (offset, string) => {
            for (let i = 0; i < string.length; i++) {
                view.setUint8(offset + i, string.charCodeAt(i));
            }
        };

        writeString(0, 'RIFF');
        view.setUint32(4, 36 + length * 2, true);
        writeString(8, 'WAVE');
        writeString(12, 'fmt ');
        view.setUint32(16, 16, true);
        view.setUint16(20, 1, true);
        view.setUint16(22, 1, true);
        view.setUint32(24, sampleRate, true);
        view.setUint32(28, sampleRate * 2, true);
        view.setUint16(32, 2, true);
        view.setUint16(34, 16, true);
        writeString(36, 'data');
        view.setUint32(40, length * 2, true);

        // 写入 PCM 数据
        let offset = 44;
        for (let i = 0; i < length; i++) {
            const sample = Math.max(-32768, Math.min(32767, pcmData[i] * 32767));
            view.setInt16(offset, sample, true);
            offset += 2;
        }

        return new Blob([buffer], { type: 'audio/wav' });
    }

    updateVADButton() {
        const vadBtn = document.getElementById('vad-btn');
        const vadIcon = document.getElementById('vad-icon');

        if (!vadBtn || !vadIcon) return;

        if (!this.vadEnabled) {
            vadBtn.classList.add('btn-disabled');
            vadBtn.title = 'VAD not available';
        } else if (this.isVadActive) {
            vadBtn.classList.remove('btn-ghost', 'btn-disabled');
            vadBtn.classList.add('btn-error');
            vadBtn.title = 'Stop Voice Activity Detection';

            // 更新图标为停止图标
            vadIcon.innerHTML = `
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 6h12v12H6z"></path>
            `;
        } else {
            vadBtn.classList.remove('btn-error', 'btn-disabled');
            vadBtn.classList.add('btn-ghost');
            vadBtn.title = 'Start Voice Activity Detection';

            // 恢复麦克风图标
            vadIcon.innerHTML = `
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11a7 7 0 01-7 7m0 0a7 7 0 01-7-7m7 7v4m0 0H8m4 0h4m-4-8a3 3 0 01-3-3V5a3 3 0 116 0v6a3 3 0 01-3 3z"></path>
            `;
        }
    }

    updateVADStatus(isListening = null) {
        const vadStatus = document.getElementById('vad-status');
        if (!vadStatus) return;

        if (isListening === true) {
            // 正在监听语音
            vadStatus.classList.remove('hidden');
            vadStatus.innerHTML = `
                <span class="loading loading-dots loading-sm mr-1"></span>
                <span>Recording...</span>
            `;
        } else if (isListening === false) {
            // 语音结束
            vadStatus.classList.remove('hidden');
            vadStatus.innerHTML = `
                <span class="mr-1">🔇</span>
                <span>Processing...</span>
            `;

            // 2秒后隐藏状态
            setTimeout(() => {
                if (this.isVadActive) {
                    vadStatus.innerHTML = `
                        <span class="loading loading-dots loading-sm mr-1"></span>
                        <span>Listening...</span>
                    `;
                } else {
                    vadStatus.classList.add('hidden');
                }
            }, 2000);
        } else if (this.isVadActive) {
            // VAD 激活但未检测到语音
            vadStatus.classList.remove('hidden');
            vadStatus.innerHTML = `
                <span class="loading loading-dots loading-sm mr-1"></span>
                <span>Listening...</span>
            `;
        } else {
            // VAD 未激活
            vadStatus.classList.add('hidden');
        }
    }

    // WebSocket 消息发送方法
    sendInput(input) {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            const message = JSON.stringify({
                type: 'input',
                input: input
            });
            this.websocket.send(message);
        }
    }

    sendBytesInput(bytes) {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            // Convert string to Uint8Array to ensure binary transmission
            const data = typeof bytes === 'string'
                ? new TextEncoder().encode(bytes)
                : bytes;
            this.websocket.send(data);
        }
    }

    sendCancel() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            const message = JSON.stringify({
                type: 'cancel'
            });
            this.websocket.send(message);
        }
    }

    sendConfirm() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            const message = JSON.stringify({
                type: 'confirm'
            });
            this.websocket.send(message);
        }
    }

    sendCreateSession() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            const message = JSON.stringify({
                type: 'create_session'
            });
            this.websocket.send(message);
        }
    }

    getCurrentState() {
        if (this.websocket && this.websocket.readyState === WebSocket.OPEN) {
            const message = JSON.stringify({
                type: 'get_current_state'
            });
            this.websocket.send(message);
        }
    }

    // 处理来自服务器的消息
    handleServerMessage(data) {
        try {
            const message = JSON.parse(data);

            switch (message.type) {
                case 'session_pty_output':
                    // PTY 输出，直接写入终端
                    if (message.output) {
                        let data = message.output.replace(/\n/g, '\r\n');
                        this.terminal.write(data);
                    }
                    break;

                case 'session_output':
                    // 会话输出，忽略终端写入，只更新思考状态
                    if (message.is_thinking) {
                        this.updateThinkingStatus(true);
                    } else {
                        this.updateThinkingStatus(false);
                    }
                    break;

                case 'session_ended':
                    console.log('Session ended:', message.session_id);
                    this.terminal.writeln('\r\n\r\n\x1b[33mSession ended\x1b[0m');
                    break;

                case 'session_running':
                    console.log('Session running:', message.session_id);
                    this.updateSessionStatus('running');
                    break;

                case 'session_idle':
                    console.log('Session idle:', message.session_id);
                    this.updateSessionStatus('idle');
                    break;

                case 'session_pending':
                    console.log('Session pending:', message.session_id, 'tool:', message.tool_name);
                    this.updateSessionStatus('pending', message.tool_name);
                    break;

                case 'session_tool_request':
                    console.log('Tool request:', message.tool_name, message.tool_input);
                    this.updateSessionStatus('tool_request', message.tool_name);
                    break;

                case 'session_error':
                    console.error('Session error:', message);
                    // error_code 是平铺的字段，根据错误类型显示不同的消息
                    let errorMsg = 'Unknown error';
                    if (message.error_code === 'invalid_input' && message.error_message) {
                        errorMsg = message.error_message;
                    } else if (message.error_code === 'invalid_input_for_state' && message.error_state) {
                        errorMsg = `Invalid input for state: ${message.error_state}`;
                    } else if (message.error_code === 'session_not_found') {
                        errorMsg = 'Session not found';
                    } else if (message.error_code === 'internal_error' && message.error_message) {
                        errorMsg = `Internal error: ${message.error_message}`;
                    }
                    this.terminal.writeln(`\r\n\x1b[31mError: ${errorMsg}\x1b[0m`);
                    break;

                default:
                    console.log('Unknown message type:', message.type, message);
            }
        } catch (e) {
            // 如果不是 JSON，当作纯文本处理
            console.log('Non-JSON message, treating as text:', data);
            let textData = data.replace(/\n/g, '\r\n');
            this.terminal.write(textData);
        }
    }

    updateThinkingStatus(isThinking) {
        const statusElement = document.getElementById('thinking-status');
        if (!statusElement) return;

        if (isThinking) {
            statusElement.classList.remove('hidden');
            statusElement.innerHTML = '<span class="loading loading-dots loading-xs"></span>';
        } else {
            statusElement.classList.add('hidden');
        }
    }

    updateSessionStatus(status, toolName = null) {
        const statusElement = document.getElementById('session-status');
        if (!statusElement) return;

        let statusHtml = '';
        switch (status) {
            case 'running':
                statusHtml = '<div class="badge badge-success"><div class="w-2 h-2 rounded-full bg-success mr-2 animate-pulse"></div>Running</div>';
                break;
            case 'idle':
                statusHtml = '<div class="badge badge-info"><div class="w-2 h-2 rounded-full bg-info mr-2"></div>Idle</div>';
                break;
            case 'pending':
                statusHtml = `<div class="badge badge-warning"><div class="w-2 h-2 rounded-full bg-warning mr-2 animate-pulse"></div>Pending: ${toolName || 'tool'}</div>`;
                break;
            case 'tool_request':
                statusHtml = `<div class="badge badge-accent"><div class="w-2 h-2 rounded-full bg-accent mr-2"></div>Tool: ${toolName || 'unknown'}</div>`;
                break;
            default:
                statusHtml = '<div class="badge badge-neutral"><div class="w-2 h-2 rounded-full bg-neutral mr-2"></div>Unknown</div>';
        }

        statusElement.innerHTML = statusHtml;
    }
}

document.addEventListener('DOMContentLoaded', () => {
    const webTerminal = new WebTerminal();

    // 将终端实例暴露到全局作用域，方便在 F12 中调试
    window.webTerminal = webTerminal;
    window.terminal = webTerminal.terminal; // 直接访问 xterm 实例

    setTimeout(() => {
        webTerminal.focus();
    }, 500);
});