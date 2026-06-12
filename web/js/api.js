/**
 * API Client for Serverust-Less
 * Provides fetch-based wrappers for all REST API endpoints
 */

const API_BASE = '/api/v1';

// Logging utility
const Logger = {
    enabled: localStorage.getItem('debug') === 'true',
    
    enable() {
        this.enabled = true;
        localStorage.setItem('debug', 'true');
        console.log('[Logger] Debug logging enabled');
    },
    
    disable() {
        this.enabled = false;
        localStorage.setItem('debug', 'false');
        console.log('[Logger] Debug logging disabled');
    },
    
    debug(...args) {
        if (this.enabled) console.debug('[DEBUG]', ...args);
    },
    
    info(...args) {
        console.info('[INFO]', ...args);
    },
    
    warn(...args) {
        console.warn('[WARN]', ...args);
    },
    
    error(...args) {
        console.error('[ERROR]', ...args);
    },
    
    group(label) {
        if (this.enabled) console.group(label);
    },
    
    groupEnd() {
        if (this.enabled) console.groupEnd();
    }
};

// Expose logger globally
window.Logger = Logger;

class APIError extends Error {
    constructor(message, status, data) {
        super(message);
        this.name = 'APIError';
        this.status = status;
        this.data = data;
    }
}

async function handleResponse(response) {
    let data;
    const contentType = response.headers.get('content-type');
    
    if (contentType && contentType.includes('application/json')) {
        data = await response.json();
    } else {
        data = await response.text();
    }
    
    if (!response.ok) {
        const message = data?.message || data?.error || `HTTP ${response.status}`;
        Logger.error('API error:', response.status, message, data);
        throw new APIError(message, response.status, data);
    }
    
    return data;
}

async function request(endpoint, options = {}) {
    const url = `${API_BASE}${endpoint}`;
    const method = options.method || 'GET';
    
    Logger.debug(`API ${method} ${endpoint}`);
    
    const config = {
        headers: {
            'Content-Type': 'application/json',
            ...options.headers,
        },
        ...options,
    };
    
    if (config.body && typeof config.body === 'object') {
        config.body = JSON.stringify(config.body);
        Logger.debug('Request body:', options.body);
    }
    
    const startTime = performance.now();
    const response = await fetch(url, config);
    const duration = (performance.now() - startTime).toFixed(1);
    
    Logger.debug(`API ${method} ${endpoint} completed in ${duration}ms (status: ${response.status})`);
    
    return handleResponse(response);
}

// ===== Jobs API =====

const JobsAPI = {
    /**
     * List all jobs with optional pagination
     */
    async list(params = {}) {
        const query = new URLSearchParams();
        if (params.limit) query.set('limit', params.limit);
        if (params.offset) query.set('offset', params.offset);
        const queryStr = query.toString();
        return request(`/jobs${queryStr ? '?' + queryStr : ''}`);
    },
    
    /**
     * Get a single job by ID
     */
    async get(id) {
        return request(`/jobs/${id}`);
    },
    
    /**
     * Create a new job
     */
    async create(job) {
        return request('/jobs', {
            method: 'POST',
            body: job,
        });
    },
    
    /**
     * Update an existing job
     */
    async update(id, job) {
        return request(`/jobs/${id}`, {
            method: 'PUT',
            body: job,
        });
    },

    /**
     * List immutable versions for a job
     */
    async listVersions(id) {
        return request(`/jobs/${id}/versions`);
    },

    /**
     * Get a specific immutable job version
     */
    async getVersion(id, version) {
        return request(`/jobs/${id}/versions/${version}`);
    },

    /**
     * Restore an older job version as the latest current version
     */
    async restoreVersion(id, version, body = {}) {
        return request(`/jobs/${id}/versions/${version}/restore`, {
            method: 'POST',
            body,
        });
    },
    
    /**
     * Delete a job
     */
    async delete(id) {
        return request(`/jobs/${id}`, {
            method: 'DELETE',
        });
    },
    
    /**
     * Execute a job
     */
    async execute(id, options = {}) {
        return request(`/jobs/${id}/execute`, {
            method: 'POST',
            body: options,
        });
    },

    /**
     * Enable a job
     */
    async enable(id) {
        return request(`/jobs/${id}/enable`, {
            method: 'POST',
        });
    },

    /**
     * Disable a job
     */
    async disable(id) {
        return request(`/jobs/${id}/disable`, {
            method: 'POST',
        });
    },
};

// ===== Executions API =====

const ExecutionsAPI = {
    /**
     * List all executions with optional filters
     */
    async list(params = {}) {
        const query = new URLSearchParams();
        if (params.limit) query.set('limit', params.limit);
        if (params.offset) query.set('offset', params.offset);
        if (params.status) query.set('status', params.status);
        if (params.job_id) query.set('job_id', params.job_id);
        const queryStr = query.toString();
        return request(`/executions${queryStr ? '?' + queryStr : ''}`);
    },
    
    /**
     * Get a single execution by ID
     */
    async get(id) {
        return request(`/executions/${id}`);
    },
    
    /**
     * Cancel a running execution
     */
    async cancel(id) {
        return request(`/executions/${id}/cancel`, {
            method: 'POST',
        });
    },

    /**
     * Retry a failed execution
     */
    async retry(id) {
        return request(`/executions/${id}/retry`, {
            method: 'POST',
        });
    },

    /**
     * Delete an execution record
     */
    async delete(id) {
        return request(`/executions/${id}`, {
            method: 'DELETE',
        });
    },

    /**
     * Bulk delete executions
     */
    async bulkDelete(ids) {
        return request('/executions/bulk', {
            method: 'DELETE',
            body: { ids },
        });
    },

    /**
     * Get execution logs
     */
    async getLogs(id, params = {}) {
        const query = new URLSearchParams();
        if (params.limit) query.set('limit', params.limit);
        if (params.offset) query.set('offset', params.offset);
        const queryStr = query.toString();
        return request(`/executions/${id}/logs${queryStr ? '?' + queryStr : ''}`);
    },
    
    /**
     * Stream execution logs via SSE
     * Returns an EventSource instance
     */
    stream(id) {
        const url = `${API_BASE}/executions/${id}/stream`;
        return new EventSource(url);
    },
};

// ===== Queue API =====

const QueueAPI = {
    /**
     * Get queue status
     */
    async status() {
        return request('/queue/status');
    },
};

// ===== Packages API =====

const PackagesAPI = {
    /**
     * List installed packages in main venv
     */
    async list() {
        return request('/packages');
    },
    
    /**
     * Install a package in main venv
     */
    async install(name, version = null) {
        return request('/packages/install', {
            method: 'POST',
            body: { name, version },
        });
    },
    
    /**
     * Uninstall a package from main venv
     */
    async uninstall(name) {
        return request('/packages/uninstall', {
            method: 'POST',
            body: { name },
        });
    },
    
    /**
     * Search PyPI for packages
     */
    async search(query) {
        return request(`/packages/search?q=${encodeURIComponent(query)}`);
    },

    /**
     * Get rich PyPI metadata for an exact package name
     */
    async getPypiDetails(name) {
        return request(`/packages/pypi/${encodeURIComponent(name)}`);
    },

    /**
     * Get main venv package list and status
     */
    async mainVenvStatus() {
        return request('/packages/main-venv');
    },
};

// ===== Venvs API =====

const VenvsAPI = {
    /**
     * List all virtual environments
     */
    async list() {
        return request('/venvs');
    },

    /**
     * Create a new named virtual environment
     */
    async create(data) {
        return request('/venvs', {
            method: 'POST',
            body: JSON.stringify(data),
        });
    },

    /**
     * Get a specific venv (includes venv details)
     */
    async get(id) {
        return request(`/venvs/${id}`);
    },

    /**
     * Delete a venv
     */
    async delete(id) {
        return request(`/venvs/${id}`, {
            method: 'DELETE',
        });
    },

    /**
     * List packages installed in a specific venv
     */
    async listPackages(id) {
        return request(`/venvs/${id}/packages`);
    },
};

// ===== Health API =====

const HealthAPI = {
    /**
     * Check API health
     */
    async check() {
        return request('/health');
    },

    /**
     * Get system statistics
     */
    async stats() {
        return request('/stats');
    },

    /**
     * Get worker pool status
     */
    async workers() {
        return request('/workers/status');
    },
};

// Export all APIs
window.API = {
    Jobs: JobsAPI,
    Executions: ExecutionsAPI,
    Queue: QueueAPI,
    Packages: PackagesAPI,
    Venvs: VenvsAPI,
    Health: HealthAPI,
    APIError,
};
