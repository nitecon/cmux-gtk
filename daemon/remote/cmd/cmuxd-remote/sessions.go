package main

import (
	"fmt"
	"sort"
	"time"
)

// These records describe client attachment sizes, not persisted child processes.
type sessionAttachment struct {
	Cols      int
	Rows      int
	UpdatedAt time.Time
}

type sessionState struct {
	attachments   map[string]sessionAttachment
	effectiveCols int
	effectiveRows int
	lastKnownCols int
	lastKnownRows int
}

// handleSessionOpen creates or retrieves attachment metadata, allocating an ID when omitted.
func (s *rpcServer) handleSessionOpen(req rpcRequest) rpcResponse {
	sessionID, _ := getStringParam(req.Params, "session_id")

	s.mu.Lock()
	defer s.mu.Unlock()

	if sessionID == "" {
		sessionID = fmt.Sprintf("sess-%d", s.nextSessionID)
		s.nextSessionID++
	}

	session, exists := s.sessions[sessionID]
	if !exists {
		session = &sessionState{
			attachments: map[string]sessionAttachment{},
		}
		s.sessions[sessionID] = session
	}

	return rpcResponse{
		ID:     req.ID,
		OK:     true,
		Result: sessionSnapshot(sessionID, session),
	}
}

// handleSessionClose removes attachment metadata and reports missing sessions explicitly.
func (s *rpcServer) handleSessionClose(req rpcRequest) rpcResponse {
	sessionID, ok := getStringParam(req.Params, "session_id")
	if !ok || sessionID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "session.close requires session_id",
			},
		}
	}

	s.mu.Lock()
	_, exists := s.sessions[sessionID]
	if exists {
		delete(s.sessions, sessionID)
	}
	s.mu.Unlock()

	if !exists {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "session not found",
			},
		}
	}

	return rpcResponse{
		ID: req.ID,
		OK: true,
		Result: map[string]any{
			"session_id": sessionID,
			"closed":     true,
		},
	}
}

// handleSessionAttach creates or updates a client's size contribution for an existing session.
func (s *rpcServer) handleSessionAttach(req rpcRequest) rpcResponse {
	return s.updateSessionAttachment(req, false)
}

// handleSessionResize changes an existing attachment without implicitly creating one.
func (s *rpcServer) handleSessionResize(req rpcRequest) rpcResponse {
	return s.updateSessionAttachment(req, true)
}

// updateSessionAttachment validates and applies a size contribution under the session lock.
// requireExisting preserves resize's missing-attachment error while attach can create it.
func (s *rpcServer) updateSessionAttachment(req rpcRequest, requireExisting bool) rpcResponse {
	method := "session.attach"
	if requireExisting {
		method = "session.resize"
	}
	sessionID, attachmentID, cols, rows, badResp := parseSessionAttachmentParams(req, method)
	if badResp != nil {
		return *badResp
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	session, exists := s.sessions[sessionID]
	if !exists {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: "not_found", Message: "session not found"}}
	}
	if _, exists := session.attachments[attachmentID]; requireExisting && !exists {
		return rpcResponse{ID: req.ID, OK: false, Error: &rpcError{Code: "not_found", Message: "attachment not found"}}
	}
	session.attachments[attachmentID] = sessionAttachment{Cols: cols, Rows: rows, UpdatedAt: time.Now().UTC()}
	recomputeSessionSize(session)
	return rpcResponse{ID: req.ID, OK: true, Result: sessionSnapshot(sessionID, session)}
}

// handleSessionDetach removes one size contribution and retains the last grid after the final detach.
func (s *rpcServer) handleSessionDetach(req rpcRequest) rpcResponse {
	sessionID, ok := getStringParam(req.Params, "session_id")
	if !ok || sessionID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "session.detach requires session_id",
			},
		}
	}
	attachmentID, ok := getStringParam(req.Params, "attachment_id")
	if !ok || attachmentID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "session.detach requires attachment_id",
			},
		}
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	session, exists := s.sessions[sessionID]
	if !exists {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "session not found",
			},
		}
	}
	if _, exists := session.attachments[attachmentID]; !exists {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "attachment not found",
			},
		}
	}

	delete(session.attachments, attachmentID)
	recomputeSessionSize(session)

	return rpcResponse{
		ID:     req.ID,
		OK:     true,
		Result: sessionSnapshot(sessionID, session),
	}
}

// handleSessionStatus returns a locked snapshot of existing session attachment metadata.
func (s *rpcServer) handleSessionStatus(req rpcRequest) rpcResponse {
	sessionID, ok := getStringParam(req.Params, "session_id")
	if !ok || sessionID == "" {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: "session.status requires session_id",
			},
		}
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	session, exists := s.sessions[sessionID]
	if !exists {
		return rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "not_found",
				Message: "session not found",
			},
		}
	}

	return rpcResponse{
		ID:     req.ID,
		OK:     true,
		Result: sessionSnapshot(sessionID, session),
	}
}

// parseSessionAttachmentParams validates both identities and positive grid dimensions for attach/resize.
func parseSessionAttachmentParams(req rpcRequest, method string) (sessionID string, attachmentID string, cols int, rows int, badResp *rpcResponse) {
	sessionID, ok := getStringParam(req.Params, "session_id")
	if !ok || sessionID == "" {
		resp := rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: method + " requires session_id",
			},
		}
		return "", "", 0, 0, &resp
	}
	attachmentID, ok = getStringParam(req.Params, "attachment_id")
	if !ok || attachmentID == "" {
		resp := rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: method + " requires attachment_id",
			},
		}
		return "", "", 0, 0, &resp
	}

	cols, ok = getIntParam(req.Params, "cols")
	if !ok || cols <= 0 {
		resp := rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: method + " requires cols > 0",
			},
		}
		return "", "", 0, 0, &resp
	}
	rows, ok = getIntParam(req.Params, "rows")
	if !ok || rows <= 0 {
		resp := rpcResponse{
			ID: req.ID,
			OK: false,
			Error: &rpcError{
				Code:    "invalid_params",
				Message: method + " requires rows > 0",
			},
		}
		return "", "", 0, 0, &resp
	}

	return sessionID, attachmentID, cols, rows, nil
}

// recomputeSessionSize selects the minimum dimensions across clients, retaining the last size when detached.
func recomputeSessionSize(session *sessionState) {
	if len(session.attachments) == 0 {
		session.effectiveCols = session.lastKnownCols
		session.effectiveRows = session.lastKnownRows
		return
	}

	minCols := 0
	minRows := 0
	for _, attachment := range session.attachments {
		if minCols == 0 || attachment.Cols < minCols {
			minCols = attachment.Cols
		}
		if minRows == 0 || attachment.Rows < minRows {
			minRows = attachment.Rows
		}
	}

	session.effectiveCols = minCols
	session.effectiveRows = minRows
	session.lastKnownCols = minCols
	session.lastKnownRows = minRows
}

// sessionSnapshot copies metadata into a deterministic response ordered by attachment ID; caller holds the lock.
func sessionSnapshot(sessionID string, session *sessionState) map[string]any {
	attachmentIDs := make([]string, 0, len(session.attachments))
	for attachmentID := range session.attachments {
		attachmentIDs = append(attachmentIDs, attachmentID)
	}
	sort.Strings(attachmentIDs)

	attachments := make([]map[string]any, 0, len(attachmentIDs))
	for _, attachmentID := range attachmentIDs {
		attachment := session.attachments[attachmentID]
		attachments = append(attachments, map[string]any{
			"attachment_id": attachmentID,
			"cols":          attachment.Cols,
			"rows":          attachment.Rows,
			"updated_at":    attachment.UpdatedAt.Format(time.RFC3339Nano),
		})
	}

	return map[string]any{
		"session_id":      sessionID,
		"attachments":     attachments,
		"effective_cols":  session.effectiveCols,
		"effective_rows":  session.effectiveRows,
		"last_known_cols": session.lastKnownCols,
		"last_known_rows": session.lastKnownRows,
	}
}
