/**
 * Venvs Component
 * Handles displaying and managing virtual environments
 */

const Venvs = {
    container: null,
    emptyState: null,
    venvs: [],
    isCreating: false,
    _buttonsSetup: false,

    init() {
        this.container = document.getElementById('venvs-list');
        this.emptyState = document.getElementById('venvs-empty');
        if (!this._buttonsSetup) {
            this._setupButtons();
            this._buttonsSetup = true;
        }
    },

    _setupButtons() {
        // New Venv button
        document.getElementById('btn-new-venv')?.addEventListener('click', () => {
            this._openCreateModal();
        });

        // Refresh button
        document.getElementById('btn-refresh-venvs')?.addEventListener('click', () => {
            this.load();
        });

        // Confirm create button
        document.getElementById('btn-confirm-create-venv')?.addEventListener('click', async () => {
            await this._createVenv();
        });

        // Enter key in name input
        document.getElementById('venv-name')?.addEventListener('keypress', async (e) => {
            if (e.key === 'Enter') await this._createVenv();
        });
    },

    _openCreateModal() {
        document.getElementById('venv-name').value = '';
        document.getElementById('venv-python-version').value = '';
        this.isCreating = false;
        const btn = document.getElementById('btn-confirm-create-venv');
        if (btn) { btn.disabled = false; btn.textContent = 'Create'; }
        Modal.open('create-venv-modal');
        setTimeout(() => document.getElementById('venv-name')?.focus(), 100);
    },

    async _createVenv() {
        if (this.isCreating) return;

        const nameInput = document.getElementById('venv-name');
        const versionInput = document.getElementById('venv-python-version');
        const name = nameInput.value.trim();

        if (!name) {
            Toast.error('Validation Error', 'Venv name is required');
            nameInput.focus();
            return;
        }
        if (!/^[a-zA-Z0-9_-]+$/.test(name)) {
            Toast.error('Validation Error', 'Name must contain only letters, numbers, hyphens, and underscores');
            nameInput.focus();
            return;
        }
        if (name === 'main') {
            Toast.error('Validation Error', "'main' is a reserved name");
            nameInput.focus();
            return;
        }

        this.isCreating = true;
        const btn = document.getElementById('btn-confirm-create-venv');
        if (btn) { btn.disabled = true; btn.textContent = 'Creating…'; }

        try {
            const payload = { name };
            const version = versionInput.value.trim();
            if (version) payload.python_version = version;

            await API.Venvs.create(payload);
            Toast.success('Venv created', `"${name}" has been created successfully`);
            Modal.close('create-venv-modal');
            await this.load();
        } catch (error) {
            Toast.error('Failed to create venv', error.message);
        } finally {
            this.isCreating = false;
            if (btn) { btn.disabled = false; btn.textContent = 'Create'; }
        }
    },

    async load() {
        this.init();

        this.container.innerHTML = `
            <div class="loading-state" style="grid-column: 1 / -1;">
                <div class="spinner"></div>
                <p>Loading virtual environments...</p>
            </div>
        `;
        this.emptyState?.classList.add('hidden');

        try {
            const response = await API.Venvs.list();
            const venvs = response.venvs || response;
            Logger.info('Loaded', venvs.length, 'venvs');
            this.venvs = venvs;
            AppState.venvs = venvs;
            this.render(venvs);
        } catch (error) {
            Toast.error('Failed to load venvs', error.message);
            this.container.innerHTML = '';
            this.emptyState?.classList.remove('hidden');
        }
    },

    render(venvs) {
        this.init();

        if (!venvs || venvs.length === 0) {
            this.container.innerHTML = '';
            this.emptyState?.classList.remove('hidden');
            return;
        }

        this.emptyState?.classList.add('hidden');
        this.container.innerHTML = venvs.map(venv => this.renderVenvCard(venv)).join('');

        // Attach event listeners
        this.container.querySelectorAll('[data-action]').forEach(btn => {
            btn.addEventListener('click', async (e) => {
                const action = e.currentTarget.dataset.action;
                const venvId = e.currentTarget.dataset.venvId;
                await this.handleAction(action, venvId);
            });
        });
    },

    _venvDisplayName(venv) {
        if (venv.venv_type === 'main') return 'main';
        // Use last segment of path as display name
        const parts = (venv.path || '').replace(/\\/g, '/').split('/');
        return parts[parts.length - 1] || venv.id;
    },

    renderVenvCard(venv) {
        const isMain = venv.venv_type === 'main';
        const displayName = this._venvDisplayName(venv);
        const statusClass = venv.status === 'ready'    ? 'badge-success'  :
                            venv.status === 'creating' ? 'badge-warning'  :
                            venv.status === 'failed'   ? 'badge-danger'   : 'badge-secondary';

        return `
            <div class="card venv-card${isMain ? ' venv-card--default' : ''}" data-venv-id="${venv.id}">
                <div class="card-header">
                    <h3 class="card-title">🐍 ${escapeHtml(displayName)}</h3>
                    <span class="badge ${statusClass}">${escapeHtml(venv.status || 'unknown')}</span>
                </div>
                <div class="card-meta">
                    ${isMain
                        ? '<span class="badge badge-info" title="Cannot be modified or deleted">🔒 Default</span>'
                        : '<span class="badge badge-warning">Custom</span>'}
                    ${venv.python_version ? `<span class="badge badge-secondary">Python ${escapeHtml(venv.python_version)}</span>` : ''}
                    ${venv.package_count > 0 ? `<span class="badge badge-secondary">📦 ${venv.package_count} pkgs</span>` : ''}
                </div>
                ${venv.path ? `
                    <p class="venv-path" title="${escapeHtml(venv.path)}">${escapeHtml(venv.path)}</p>
                ` : ''}
                ${venv.error_message ? `
                    <p class="venv-error">⚠️ ${escapeHtml(venv.error_message)}</p>
                ` : ''}
                <div class="card-actions">
                    <div class="card-actions-primary">
                        <button class="btn btn-secondary btn-sm" data-action="view" data-venv-id="${venv.id}">
                            👁️ Details
                        </button>
                    </div>
                    <div class="card-actions-secondary">
                        ${!isMain ? `
                            <button class="btn btn-danger btn-icon" data-action="delete" data-venv-id="${venv.id}" title="Delete">
                                🗑️
                            </button>
                        ` : ''}
                    </div>
                </div>
            </div>
        `;
    },

    async handleAction(action, venvId) {
        switch (action) {
            case 'view':
                await this.viewPackages(venvId);
                break;
            case 'delete':
                await this.deleteVenv(venvId);
                break;
        }
    },

    async viewPackages(venvId) {
        try {
            const venv = await API.Venvs.get(venvId);
            const name = this._venvDisplayName(venv);
            const isMain = venv.venv_type === 'main';

            const statusClass = venv.status === 'ready'    ? 'badge-success'  :
                                venv.status === 'creating' ? 'badge-warning'  :
                                venv.status === 'failed'   ? 'badge-danger'   : 'badge-secondary';

            const rows = [
                ['Name',       `${escapeHtml(name)}${isMain ? ' <span class="badge badge-info">🔒 Default</span>' : ''}`],
                ['Type',       escapeHtml(venv.venv_type || 'unknown')],
                ['Status',     `<span class="badge ${statusClass}">${escapeHtml(venv.status || 'unknown')}</span>`],
                ['Python',     escapeHtml(venv.python_version || '—')],
                ['Packages',   venv.package_count != null ? venv.package_count : '—'],
                ['Path',       `<span class="venv-path" title="${escapeHtml(venv.path || '')}">${escapeHtml(venv.path || '—')}</span>`],
                ['Created',    venv.created_at   ? new Date(venv.created_at).toLocaleString()   : '—'],
                ['Last used',  venv.last_used_at ? new Date(venv.last_used_at).toLocaleString() : '—'],
            ];

            const tableHtml = `
                <table class="venv-details-table">
                    <tbody>
                        ${rows.map(([label, value]) => `
                        <tr>
                            <th>${label}</th>
                            <td>${value}</td>
                        </tr>`).join('')}
                    </tbody>
                </table>
                ${venv.error_message ? `<p class="venv-error" style="margin-top:var(--spacing-md)">⚠️ ${escapeHtml(venv.error_message)}</p>` : ''}
            `;

            document.getElementById('venv-details-title').textContent = `Virtual Environment: ${name}`;
            document.getElementById('venv-details-body').innerHTML = tableHtml;
            Modal.open('venv-details-modal');
        } catch (error) {
            Toast.error('Failed to load venv details', error.message);
        }
    },

    async deleteVenv(venvId) {
        const venv = this.venvs.find(v => v.id === venvId);
        if (!venv) return;

        // Guard: never delete main
        if (venv.venv_type === 'main') {
            Toast.error('Not allowed', 'The Default (main) venv cannot be deleted');
            return;
        }

        const name = this._venvDisplayName(venv);
        const ok = await Confirm.show(`Are you sure you want to delete "${name}"?\n\nThis will remove the virtual environment and all its packages.`, { title: 'Delete Venv', confirmText: 'Delete' });
        if (!ok) return;

        try {
            await API.Venvs.delete(venvId);
            Toast.success('Deleted', `"${name}" has been deleted`);
            this.load();
        } catch (error) {
            Toast.error('Failed to delete', error.message);
        }
    },
};
