// GIF Replacer Tool - Frontend
console.log('app.js loaded');

// Wait for DOM and Tauri to be ready
document.addEventListener('DOMContentLoaded', async () => {
    console.log('DOM loaded, initializing app...');
    await initApp();
});

async function initApp() {
    console.log('initApp called');
    // Check if Tauri API is available
    if (!window.__TAURI__) {
        console.error('Tauri API not available!');
        alert('Error: Tauri API not loaded. Please restart the app.');
        return;
    }

    console.log('Tauri API available:', window.__TAURI__);

    const invoke = window.__TAURI__.tauri.invoke;
    const listen = window.__TAURI__.event.listen;

    // State
    let currentFile = null;
    let currentEmotion = null;
    let config = null;
    let emotions = [];
    let symbols = [];
    let activeProfile = null;

    // DOM Elements
    const projectPathInput = document.getElementById('project-path-input');
    const browseProjectBtn = document.getElementById('browse-project-btn');
    const profileSelector = document.getElementById('profile-selector');
    const profileSelect = document.getElementById('profile-select');
    const profileName = document.getElementById('profile-name');
    const serialPortSelect = document.getElementById('serial-port-select');
    const scanPortsBtn = document.getElementById('scan-ports-btn');
    const serialPortInput = document.getElementById('serial-port');

    const dropArea = document.getElementById('drop-area');
    const dropPlaceholder = document.getElementById('drop-placeholder');
    const fileInfo = document.getElementById('file-info');
    const fileName = document.getElementById('file-name');
    const fileSize = document.getElementById('file-size');
    const clearFileBtn = document.getElementById('clear-file-btn');

    const emotionsContainer = document.getElementById('emotions-container');
    const defaultEmotionSection = document.getElementById('default-emotion-section');
    const defaultEmotionSelect = document.getElementById('default-emotion-select');
    const setDefaultBtn = document.getElementById('set-default-btn');
    const cloneProfileBtn = document.getElementById('clone-profile-btn');
    const resetDefaultBtn = document.getElementById('reset-default-btn');
    const statusDiv = document.getElementById('status');
    const runBtn = document.getElementById('run-btn');
    const outputLog = document.getElementById('output-log');

    const confirmModal = document.getElementById('confirm-modal');
    const confirmTitle = document.getElementById('confirm-title');
    const confirmBody = document.getElementById('confirm-body');
    const confirmOk = document.getElementById('confirm-ok');
    const confirmCancel = document.getElementById('confirm-cancel');

    // Initialize
    async function init() {
        try {
            config = await invoke('load_config');

            // Load saved project path
            if (config.project_path) {
                projectPathInput.value = config.project_path;
                await loadProfile();
            }

            // Load saved serial port
            if (config.last_serial_port) {
                serialPortInput.value = config.last_serial_port;
            }

            // Auto-scan ports on startup
            await scanPorts();
        } catch (err) {
            console.error('Init error:', err);
            logOutput('Error during initialization: ' + err);
        }
    }

    // Scan for available serial ports
    async function scanPorts() {
        try {
            console.log('Scanning for serial ports...');
            const ports = await invoke('list_serial_ports');
            console.log('Found ports:', ports);

            serialPortSelect.innerHTML = '<option value="">Select a port...</option>';

            if (ports.length === 0) {
                const option = document.createElement('option');
                option.value = '';
                option.textContent = 'No ports found';
                option.disabled = true;
                serialPortSelect.appendChild(option);
                logOutput('⚠ No serial ports found. Connect your device and click Scan.');
            } else {
                ports.forEach(port => {
                    const option = document.createElement('option');
                    option.value = port.port_name;
                    option.textContent = `${port.port_name} (${port.port_type})`;
                    serialPortSelect.appendChild(option);
                });

                // Auto-select the saved port if it exists in the list
                if (config.last_serial_port) {
                    const portExists = ports.some(p => p.port_name === config.last_serial_port);
                    if (portExists) {
                        serialPortSelect.value = config.last_serial_port;
                    }
                }

                logOutput(`✓ Found ${ports.length} serial port(s)`);
            }

            updateRunButton();
        } catch (err) {
            console.error('Port scan error:', err);
            logOutput('❌ Failed to scan ports: ' + err);
        }
    }

    // Load profile and emotions
    async function loadProfile() {
        try {
            console.log('Loading profile for path:', projectPathInput.value.trim());

            // First, list available profiles
            const availableProfiles = await invoke('list_available_profiles', {
                projectPath: projectPathInput.value.trim()
            });

            console.log('Available profiles:', availableProfiles);

            // Populate profile selector
            if (availableProfiles.length > 0) {
                profileSelect.innerHTML = '';
                availableProfiles.forEach(profile => {
                    const option = document.createElement('option');
                    option.value = profile;
                    option.textContent = profile;
                    profileSelect.appendChild(option);
                });
                profileSelector.style.display = 'block';
            } else {
                profileSelector.style.display = 'none';
            }

            // Get active profile
            const result = await invoke('get_active_profile', {
                projectPath: projectPathInput.value.trim()
            });

            console.log('Profile loaded successfully:', result);

            // Set the active profile in the dropdown
            profileSelect.value = result.active_profile;
            activeProfile = result.active_profile;
            cloneProfileBtn.disabled = false;
            resetDefaultBtn.style.display = /_\d+$/.test(activeProfile) ? 'inline-block' : 'none';

            profileName.textContent = `✓ Loaded profile: ${result.active_profile} (${result.emotions.length} emotions)`;
            profileName.classList.remove('hint', 'error-text');
            profileName.classList.add('success-text');
            emotions = result.emotions;
            symbols = result.symbols || [];
            renderEmotions(emotions);
            renderDefaultEmotionOptions(symbols);

            // Load default emotion
            await loadDefaultEmotion();

            // Clear any previously loaded file since emotions changed
            if (currentFile) {
                clearFile();
            }
        } catch (err) {
            console.error('Failed to load profile:', err);
            profileName.textContent = `Error: ${err}`;
            profileName.classList.remove('success-text');
            profileName.classList.add('error-text');
            emotionsContainer.innerHTML = `<p class="hint error">Failed to load profile. Make sure the path points to the EmotionDisplay project root.</p>`;
            profileSelector.style.display = 'none';
            emotions = [];
            symbols = [];
            activeProfile = null;
            cloneProfileBtn.disabled = true;
            resetDefaultBtn.style.display = 'none';
        }
    }

    // Render emotion radio buttons (gif_table keys — used to pick a replace slot)
    function renderEmotions(emotionsList) {
        if (emotionsList.length === 0) {
            emotionsContainer.innerHTML = '<p class="hint">No emotions found</p>';
            return;
        }

        const grid = document.createElement('div');
        grid.className = 'emotion-grid';

        emotionsList.forEach(emotion => {
            const label = document.createElement('label');
            label.className = 'emotion-option';

            const radio = document.createElement('input');
            radio.type = 'radio';
            radio.name = 'emotion';
            radio.value = emotion;
            radio.addEventListener('change', () => {
                currentEmotion = emotion;
                updateRunButton();
            });

            const text = document.createTextNode(emotion);

            label.appendChild(radio);
            label.appendChild(text);
            grid.appendChild(label);
        });

        emotionsContainer.innerHTML = '';
        emotionsContainer.appendChild(grid);
    }

    // Populate the default-emotion dropdown from the profile's C symbols
    // (the names valid for `#define GIF_PROFILE_DEFAULT <name>`), NOT from the
    // gif_table[] keys. gif_table maps keys like "startup"/"standby" onto a
    // handful of shared symbols, and only symbols are valid defaults.
    function renderDefaultEmotionOptions(symbolsList) {
        if (symbolsList.length === 0) {
            defaultEmotionSection.style.display = 'none';
            return;
        }
        defaultEmotionSection.style.display = 'block';
        defaultEmotionSelect.innerHTML = '';
        symbolsList.forEach(symbol => {
            const option = document.createElement('option');
            option.value = symbol;
            option.textContent = symbol;
            defaultEmotionSelect.appendChild(option);
        });
    }

    // Load the current default emotion
    async function loadDefaultEmotion() {
        if (!config.project_path) return;

        try {
            const defaultEmotion = await invoke('get_default_emotion', {
                projectPath: config.project_path
            });
            console.log('Current default emotion:', defaultEmotion);
            defaultEmotionSelect.value = defaultEmotion;
        } catch (err) {
            console.error('Failed to load default emotion:', err);
        }
    }

    // File handling — dispatches on extension
    async function handleFile(filePath) {
        const ext = filePath.split('.').pop().toLowerCase();
        const baseName = filePath.split('/').pop();

        try {
            if (ext === 'gif') {
                const result = await invoke('validate_gif_file', { filePath });
                currentFile = filePath;
                fileName.textContent = `${baseName} (${result.width}×${result.height})`;
                fileSize.textContent = result.file_size_bytes;
                logOutput(`✓ Loaded GIF: ${result.width}×${result.height}`);
            } else if (ext === 'c') {
                const result = await invoke('validate_c_file', { filePath });
                currentFile = filePath;
                fileName.textContent = result.original_name;
                fileSize.textContent = result.file_size_bytes;
                logOutput(`✓ Loaded file: ${result.original_name}`);
            } else {
                throw new Error('Drop a .gif or .c file.');
            }

            dropPlaceholder.style.display = 'none';
            fileInfo.style.display = 'block';
            updateRunButton();
        } catch (err) {
            logOutput(`❌ Invalid file: ${err}`);
            alert(`Invalid file: ${err}`);
        }
    }

    function clearFile() {
        currentFile = null;
        dropPlaceholder.style.display = 'block';
        fileInfo.style.display = 'none';
        updateRunButton();
    }

    // Update run button state
    function updateRunButton() {
        const selectedPort = serialPortSelect.value || serialPortInput.value.trim();
        const isReady = currentFile && currentEmotion && selectedPort;
        runBtn.disabled = !isReady;
    }

    // Step 1: Project path handling
    projectPathInput.addEventListener('input', updateRunButton);

    projectPathInput.addEventListener('change', async () => {
        const newPath = projectPathInput.value.trim();
        if (newPath && newPath !== config.project_path) {
            config.project_path = newPath;
            try {
                await invoke('save_config', { config });
                await loadProfile();
            } catch (err) {
                console.error('Failed to save/load:', err);
            }
        }
    });

    browseProjectBtn.addEventListener('click', async () => {
        try {
            const selected = await invoke('browse_folder');
            if (selected) {
                projectPathInput.value = selected;
                config.project_path = selected;
                await invoke('save_config', { config });
                await loadProfile();
            }
        } catch (err) {
            console.error('Browse error:', err);
        }
    });

    // Handle profile selection change
    profileSelect.addEventListener('change', async () => {
        const selectedProfile = profileSelect.value;
        if (!selectedProfile || !config.project_path) return;

        try {
            console.log('Changing profile to:', selectedProfile);
            await invoke('set_active_profile', {
                projectPath: config.project_path,
                profileName: selectedProfile
            });

            // Reload to get new emotions
            await loadProfile();
            logOutput(`✓ Switched to profile: ${selectedProfile}`);
        } catch (err) {
            console.error('Failed to change profile:', err);
            alert('Failed to change profile: ' + err);
            // Reload to reset the dropdown to the actual active profile
            await loadProfile();
        }
    });

    // Clone the active profile's base into a numbered clone
    cloneProfileBtn.addEventListener('click', async () => {
        if (!config.project_path || !activeProfile) return;
        const base = activeProfile.replace(/_\d+$/, '');
        cloneProfileBtn.disabled = true;
        try {
            const newName = await invoke('create_profile_clone', {
                projectPath: config.project_path,
                baseName: base,
            });
            logOutput(`✓ Created clone: ${newName}`);
            await loadProfile();
        } catch (err) {
            logOutput(`❌ Clone failed: ${err}`);
            alert(`Clone failed: ${err}`);
        } finally {
            cloneProfileBtn.disabled = false;
        }
    });

    // Reset the active clone's default emotion to its base's value
    resetDefaultBtn.addEventListener('click', async () => {
        if (!config.project_path || !activeProfile) return;
        resetDefaultBtn.disabled = true;
        try {
            await invoke('reset_profile_to_default', {
                projectPath: config.project_path,
                profileName: activeProfile,
            });
            await loadDefaultEmotion();
            logOutput('✓ Reset default emotion to the base profile\'s default');
        } catch (err) {
            logOutput(`❌ Reset failed: ${err}`);
            alert(`Reset failed: ${err}`);
        } finally {
            resetDefaultBtn.disabled = false;
        }
    });

    // Change the default startup emotion (write header + build & flash so it
    // actually reaches the board)
    setDefaultBtn.addEventListener('click', async () => {
        if (!config.project_path) {
            alert('Please set the project path first');
            return;
        }
        const emotion = defaultEmotionSelect.value;
        if (!emotion) {
            alert('Please select a default emotion');
            return;
        }
        const selectedPort = serialPortSelect.value || serialPortInput.value.trim();
        if (!selectedPort) {
            alert('Please select a serial port (needed to flash the change)');
            return;
        }

        setDefaultBtn.disabled = true;
        setStatus('running', 'Setting default + building & flashing...');
        outputLog.textContent = '';

        // Save serial port to config
        config.last_serial_port = selectedPort;
        try {
            await invoke('save_config', { config });
        } catch (err) {
            console.error('Failed to save serial port:', err);
        }

        try {
            await invoke('set_default_emotion', {
                projectPath: config.project_path,
                emotion,
            });
            logOutput(`✓ Default startup emotion set to: ${emotion}`);
            await loadDefaultEmotion();

            const result = await invoke('build_and_flash', {
                projectPath: config.project_path,
                serialPort: selectedPort,
            });
            if (result.success) {
                setStatus('success', result.message);
            } else {
                setStatus('error', result.message);
            }
        } catch (err) {
            setStatus('error', 'Error: ' + err);
            logOutput('❌ ' + err);
        } finally {
            setDefaultBtn.disabled = false;
        }
    });

    // Step 3: Drag and drop handlers
    dropArea.addEventListener('click', async () => {
        console.log('Drop area clicked');
        try {
            const { open } = window.__TAURI__.dialog;
            console.log('Opening file dialog...');
            const selected = await open({
                multiple: false,
                filters: [{
                    name: 'GIF or C Files',
                    extensions: ['gif', 'c']
                }]
            });
            console.log('Selected file:', selected);
            if (selected) {
                await handleFile(selected);
            }
        } catch (err) {
            console.error('Browse error:', err);
            alert('Browse error: ' + err);
        }
    });

    dropArea.addEventListener('dragover', (e) => {
        console.log('Drag over');
        e.preventDefault();
        dropArea.classList.add('drag-over');
    });

    dropArea.addEventListener('dragleave', () => {
        console.log('Drag leave');
        dropArea.classList.remove('drag-over');
    });

    dropArea.addEventListener('drop', async (e) => {
        console.log('Drop event:', e);
        e.preventDefault();
        dropArea.classList.remove('drag-over');

        if (e.dataTransfer.files.length > 0) {
            const file = e.dataTransfer.files[0];
            console.log('Dropped file:', file);
            if (file.path) {
                await handleFile(file.path);
            }
        }
    });

    // Listen for file drops from Tauri
    console.log('Registering tauri://file-drop listener');
    await listen('tauri://file-drop', async (event) => {
        console.log('File dropped via Tauri:', event);
        if (event.payload && event.payload.length > 0) {
            const filePath = event.payload[0];
            await handleFile(filePath);
        }
    });

    clearFileBtn.addEventListener('click', clearFile);

    // Serial port selection
    serialPortSelect.addEventListener('change', () => {
        // Sync the dropdown selection to the manual input field
        if (serialPortSelect.value) {
            serialPortInput.value = serialPortSelect.value;
        }
        updateRunButton();
    });

    // Manual serial port input
    serialPortInput.addEventListener('input', () => {
        // If user types manually, clear the dropdown selection
        serialPortSelect.value = '';
        updateRunButton();
    });

    // Scan ports button
    scanPortsBtn.addEventListener('click', async () => {
        await scanPorts();
    });

    // Main action
    runBtn.addEventListener('click', async () => {
        const selectedPort = serialPortSelect.value || serialPortInput.value.trim();

        if (!currentFile || !currentEmotion || !selectedPort) {
            return;
        }

        if (!config.project_path) {
            alert('Please set the EmotionDisplay project path first');
            return;
        }

        // Flash budget guard: only meaningful for a .gif, which is the only
        // input whose size we don't already know to be correct.
        if (currentFile.toLowerCase().endsWith('.gif')) {
            let budget = null;
            try {
                budget = await invoke('check_flash_budget', {
                    projectPath: config.project_path,
                    targetEmotion: currentEmotion,
                    gifPath: currentFile,
                });
            } catch (err) {
                // We could not even compute the budget. Warn and carry on —
                // this is the genuinely advisory case.
                logOutput(`⚠ Could not check the flash budget: ${err}`);
            }

            if (budget && budget.overflows) {
                const kb = (n) => Math.round(n / 1024);
                const h = budget.headroom;
                let headroom;
                if (h.state === 'NeverBuilt') {
                    headroom = 'an unknown amount (this project has not been built yet)';
                } else if (h.state === 'Stale') {
                    headroom = 'an unknown amount (the last build predates your latest changes)';
                } else if (h.bytes < 0) {
                    // The last build failed on partition size but still left
                    // its .bin behind, so it is not a usable baseline.
                    headroom = `${kb(-h.bytes)} KB OVER the partition ` +
                               `(the last build did not fit)`;
                } else {
                    headroom = `~${kb(h.bytes)} KB`;
                }
                const sign = budget.delta_bytes >= 0 ? '+' : '';

                const message =
                    `This GIF is ${kb(budget.incoming_bytes)} KB.\n` +
                    `The file it replaces holds ${kb(budget.current_bytes)} KB.\n\n` +
                    `That is ${sign}${kb(budget.delta_bytes)} KB against ${headroom} ` +
                    `free in the app partition.\n\n` +
                    `The build will overflow unless you free space first ` +
                    `(see the README on nullptr gif_table entries).`;

                logOutput(`⚠ Flash budget: ${message.replace(/\n+/g, ' ')}`);

                const proceed = await confirmInApp('Flash budget', message);

                if (!proceed) {
                    logOutput('ℹ Cancelled at the flash budget prompt.');
                    return;
                }
            }
        }

        runBtn.disabled = true;
        setStatus('running', 'Running...');
        outputLog.textContent = '';

        // Save serial port to config
        config.last_serial_port = selectedPort;
        try {
            await invoke('save_config', { config });
        } catch (err) {
            console.error('Failed to save serial port:', err);
        }

        try {
            const result = await invoke('replace_and_build_flash', {
                projectPath: config.project_path,
                sourceFilePath: currentFile,
                targetEmotion: currentEmotion,
                serialPort: selectedPort,
            });

            if (result.success) {
                setStatus('success', result.message);
            } else {
                setStatus('error', result.message);
            }
        } catch (err) {
            setStatus('error', 'Error: ' + err);
            logOutput('❌ ' + err);
        } finally {
            runBtn.disabled = false;
            updateRunButton();
        }
    });

    // Listen for build output
    listen('build-output', (event) => {
        logOutput(event.payload.line);
    });

    // Helpers
    function setStatus(type, message) {
        statusDiv.className = `status ${type}`;
        statusDiv.textContent = message;
    }

    // In-app confirmation. Deliberately NOT window.__TAURI__.dialog.ask:
    // that API only exists when the tauri `dialog-ask` Cargo feature is
    // compiled in, and when it isn't the budget guard silently loses its
    // voice (or worse, blocks every run). This always works.
    function confirmInApp(title, body) {
        return new Promise((resolve) => {
            confirmTitle.textContent = title;
            confirmBody.textContent = body;
            confirmModal.style.display = 'flex';

            function finish(result) {
                confirmModal.style.display = 'none';
                confirmOk.removeEventListener('click', onOk);
                confirmCancel.removeEventListener('click', onCancel);
                resolve(result);
            }
            function onOk() { finish(true); }
            function onCancel() { finish(false); }

            confirmOk.addEventListener('click', onOk);
            confirmCancel.addEventListener('click', onCancel);
        });
    }

    function logOutput(message) {
        outputLog.textContent += message + '\n';
        outputLog.scrollTop = outputLog.scrollHeight;
    }

    // Start
    init();
}
