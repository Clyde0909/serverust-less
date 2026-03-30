/**
 * Main Application Entry Point
 * Handles routing, state management, and UI coordination
 */

// ===== Application State =====
const AppState = {
    currentView: 'jobs',
    jobs: [],
    executions: [],
    packages: [],
    venvs: [],
    selectedJob: null,
    selectedExecution: null,
    refreshInterval: null,
};

// ===== Toast Notifications =====
const Toast = {
    container: null,
    
    init() {
        this.container = document.getElementById('toast-container');
    },
    
    show(type, title, message = '', duration = 4000) {
        Logger.debug(`Toast [${type}]: ${title}`, message || '');
        
        const icons = {
            success: '✅',
            error: '❌',
            warning: '⚠️',
            info: 'ℹ️',
        };
        
        const toast = document.createElement('div');
        toast.className = `toast toast-${type}`;
        toast.innerHTML = `
            <span class="toast-icon">${icons[type]}</span>
            <div class="toast-content">
                <div class="toast-title">${title}</div>
                ${message ? `<div class="toast-message">${message}</div>` : ''}
            </div>
        `;
        
        this.container.appendChild(toast);
        
        setTimeout(() => {
            toast.classList.add('toast-exit');
            setTimeout(() => toast.remove(), 200);
        }, duration);
    },
    
    success(title, message) { this.show('success', title, message); },
    error(title, message) { this.show('error', title, message); },
    warning(title, message) { this.show('warning', title, message); },
    info(title, message) { this.show('info', title, message); },
};

// ===== Confirm Dialog =====
const Confirm = {
    _resolve: null,

    show(message, { title = 'Confirm', confirmText = 'Confirm', danger = true } = {}) {
        return new Promise((resolve) => {
            this._resolve = resolve;

            document.getElementById('confirm-modal-title').textContent = title;
            document.getElementById('confirm-modal-message').textContent = message;

            const okBtn = document.getElementById('confirm-modal-ok');
            okBtn.textContent = confirmText;
            okBtn.className = `btn ${danger ? 'btn-danger' : 'btn-primary'}`;

            Modal.open('confirm-modal');
        });
    },

    _init() {
        document.getElementById('confirm-modal-ok')?.addEventListener('click', () => {
            Modal.close('confirm-modal');
            if (this._resolve) { this._resolve(true); this._resolve = null; }
        });
        document.getElementById('confirm-modal-cancel')?.addEventListener('click', () => {
            Modal.close('confirm-modal');
            if (this._resolve) { this._resolve(false); this._resolve = null; }
        });
        // backdrop click = cancel
        document.querySelector('#confirm-modal .modal-backdrop')?.addEventListener('click', () => {
            Modal.close('confirm-modal');
            if (this._resolve) { this._resolve(false); this._resolve = null; }
        });
    },
};

// ===== Modal Management =====
const Modal = {
    open(modalId) {
        Logger.debug('Opening modal:', modalId);
        const modal = document.getElementById(modalId);
        if (modal) {
            modal.classList.remove('hidden');
            document.body.style.overflow = 'hidden';
        }
    },
    
    close(modalId) {
        const modal = document.getElementById(modalId);
        if (modal) {
            modal.classList.add('hidden');
            document.body.style.overflow = '';
        }
    },
    
    closeAll() {
        document.querySelectorAll('.modal').forEach(modal => {
            modal.classList.add('hidden');
        });
        document.body.style.overflow = '';
    },
};

// ===== Navigation =====
function navigateTo(viewName) {
    Logger.info('Navigating to:', viewName);
    
    // Update nav links
    document.querySelectorAll('.nav-link').forEach(link => {
        link.classList.toggle('active', link.dataset.view === viewName);
    });
    
    // Show/hide views
    document.querySelectorAll('.view').forEach(view => {
        view.classList.toggle('active', view.id === `view-${viewName}`);
    });
    
    AppState.currentView = viewName;
    
    // Load data for the view
    switch (viewName) {
        case 'jobs':
            JobList.load();
            break;
        case 'executions':
            ExecutionHistory.load();
            break;
        case 'packages':
            Packages.load();
            break;
        case 'venvs':
            Venvs.load();
            break;
    }
}

// ===== Auto-Refresh =====
function startAutoRefresh() {
    stopAutoRefresh();
    Logger.debug('Starting auto-refresh (5s interval)');
    AppState.refreshInterval = setInterval(() => {
        Logger.debug('Auto-refresh triggered for:', AppState.currentView);
        switch (AppState.currentView) {
            case 'jobs':
                JobList.load(true);
                break;
            case 'executions':
                ExecutionHistory.load(true);
                break;
        }
    }, 5000);
}

function stopAutoRefresh() {
    if (AppState.refreshInterval) {
        clearInterval(AppState.refreshInterval);
        AppState.refreshInterval = null;
    }
}

// ===== Utility Functions =====
function formatDate(dateStr) {
    if (!dateStr) return '-';
    const date = new Date(dateStr);
    return date.toLocaleString();
}

function formatDuration(startStr, endStr) {
    if (!startStr) return '-';
    const start = new Date(startStr);
    const end = endStr ? new Date(endStr) : new Date();
    const durationMs = end - start;
    
    if (durationMs < 1000) {
        return `${durationMs}ms`;
    } else if (durationMs < 60000) {
        return `${(durationMs / 1000).toFixed(1)}s`;
    } else {
        const mins = Math.floor(durationMs / 60000);
        const secs = ((durationMs % 60000) / 1000).toFixed(0);
        return `${mins}m ${secs}s`;
    }
}

function getStatusBadgeClass(status) {
    return `badge status-${status}`;
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function debounce(func, wait) {
    let timeout;
    return function executedFunction(...args) {
        const later = () => {
            clearTimeout(timeout);
            func(...args);
        };
        clearTimeout(timeout);
        timeout = setTimeout(later, wait);
    };
}

// ===== Event Handlers =====
function setupEventListeners() {
    // Navigation
    document.querySelectorAll('.nav-link').forEach(link => {
        link.addEventListener('click', (e) => {
            e.preventDefault();
            navigateTo(link.dataset.view);
        });
    });
    
    // Refresh button
    document.getElementById('btn-refresh')?.addEventListener('click', () => {
        navigateTo(AppState.currentView);
        Toast.info('Refreshed', 'Data has been refreshed');
    });
    
    // Init confirm dialog
    Confirm._init();

    // Modal close handlers
    document.querySelectorAll('[data-close-modal]').forEach(btn => {
        btn.addEventListener('click', () => {
            Modal.closeAll();
        });
    });
    
    // Close modal on backdrop click
    document.querySelectorAll('.modal-backdrop').forEach(backdrop => {
        backdrop.addEventListener('click', () => {
            Modal.closeAll();
        });
    });
    
    // Close modal on Escape key
    document.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            Modal.closeAll();
        }
    });
    
    // New job button
    document.getElementById('btn-new-job')?.addEventListener('click', () => {
        JobForm.openNew();
    });
    
    // Job search
    const jobSearch = document.getElementById('job-search');
    if (jobSearch) {
        jobSearch.addEventListener('input', debounce((e) => {
            JobList.filter(e.target.value);
        }, 300));
    }
    
    // Execution status filter
    document.getElementById('execution-status-filter')?.addEventListener('change', (e) => {
        ExecutionHistory.filterByStatus(e.target.value);
    });
    
    // Clear completed executions
    document.getElementById('btn-clear-completed')?.addEventListener('click', () => {
        ExecutionHistory.clearCompleted();
    });
    
    // Install package button
    document.getElementById('btn-install-package')?.addEventListener('click', () => {
        Modal.open('package-modal');
    });
    
    // Package search input
    let searchTimeout;
    document.getElementById('package-search')?.addEventListener('input', async (e) => {
        const query = e.target.value.trim();
        
        // Clear previous timeout
        if (searchTimeout) {
            clearTimeout(searchTimeout);
        }
        
        // Don't search if query is too short
        if (query.length < 2) {
            return;
        }
        
        // Debounce search by 300ms
        searchTimeout = setTimeout(async () => {
            try {
                const results = await Packages.searchPackages(query);
                
                // Show search results as a dropdown or modal
                if (results && results.length > 0) {
                    // Pre-fill the install modal with the first result
                    document.getElementById('pkg-name').value = results[0].name;
                    if (results[0].version) {
                        document.getElementById('pkg-version').value = results[0].version;
                    }
                    Toast.info('Found', `Package: ${results[0].name} v${results[0].version}`);
                } else {
                    Toast.warning('No results', `No package found for "${query}"`);
                }
            } catch (error) {
                console.error('Search failed:', error);
            }
        }, 300);
    });
    
    // Confirm package install
    document.getElementById('btn-confirm-install')?.addEventListener('click', async () => {
        const name = document.getElementById('pkg-name').value.trim();
        const version = document.getElementById('pkg-version').value.trim() || null;
        
        if (!name) {
            Toast.error('Error', 'Package name is required');
            return;
        }
        
        try {
            await API.Packages.install(name, version);
            Toast.success('Success', `Package ${name} installed`);
            Modal.close('package-modal');
            document.getElementById('pkg-name').value = '';
            document.getElementById('pkg-version').value = '';
            Packages.load();
        } catch (error) {
            Toast.error('Failed to install', error.message);
        }
    });
    
    // Refresh venvs button
    document.getElementById('btn-refresh-venvs')?.addEventListener('click', () => {
        Venvs.load();
    });
    
    // Output tabs
    document.querySelectorAll('.tab-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            const tabName = e.target.dataset.tab;
            const modal = e.target.closest('.modal');
            
            modal.querySelectorAll('.tab-btn').forEach(t => {
                t.classList.toggle('active', t.dataset.tab === tabName);
            });
            
            modal.querySelectorAll('.tab-content').forEach(content => {
                content.classList.toggle('active', content.id === `exec-${tabName}`);
            });
        });
    });
}

// ===== Initialize Application =====
async function initApp() {
    Toast.init();
    setupEventListeners();
    
    // Check API health
    try {
        await API.Health.check();
        Toast.success('Connected', 'API is online');
    } catch (error) {
        Toast.error('API Error', 'Could not connect to API');
    }
    
    // Load initial view
    navigateTo('jobs');
    
    // Start auto-refresh
    startAutoRefresh();
}

// Start the app when DOM is ready
document.addEventListener('DOMContentLoaded', initApp);
