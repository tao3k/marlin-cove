// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use marlin_cove::marlin_gateway::{MarlinGatewayClient, MarlinGatewayError};
use marlin_cove::protocol::{
    BridgeOrigin, ChannelId, GateApprovalReceipt, GateDecision, GateId, GateResolutionInput,
    PresenceStatus, PrincipalId, PrincipalKind, PrincipalRole, ProtocolError, RoomId,
    RoomParticipant, ScopeId, extract_mentions,
};
use pretty_assertions::assert_eq;

#[test]
fn domain_identifiers_reject_blank_values() {
    assert_eq!(
        RoomId::new("   "),
        Err(ProtocolError::EmptyIdentifier { field: "room_id" })
    );
    assert_eq!(RoomId::new("  cove-main  ").unwrap().as_str(), "cove-main");
}

#[test]
fn gate_approval_receipt_enforces_human_only_invariant() {
    let gate_id = GateId::new("gate_01").unwrap();
    let bot_id = PrincipalId::new("agent:architect").unwrap();
    let human_id = PrincipalId::new("human:alice").unwrap();

    let rejected = GateApprovalReceipt::resolve_by_human(GateResolutionInput {
        gate_id: gate_id.clone(),
        actor_id: bot_id.clone(),
        actor_kind: PrincipalKind::Agent,
        approved: true,
        reason: "self-approve attempt".to_string(),
        resolved_at_unix: 1_700_000_000,
    });
    assert_eq!(
        rejected,
        Err(ProtocolError::HumanApprovalRequired {
            gate_id: gate_id.clone(),
            actor_id: bot_id,
        })
    );

    let approved = GateApprovalReceipt::resolve_by_human(GateResolutionInput {
        gate_id: gate_id.clone(),
        actor_id: human_id.clone(),
        actor_kind: PrincipalKind::Human,
        approved: true,
        reason: "verified diff".to_string(),
        resolved_at_unix: 1_700_000_010,
    })
    .unwrap();
    assert_eq!(approved.gate_id, gate_id);
    assert_eq!(approved.resolved_by, human_id);
    assert_eq!(approved.decision, GateDecision::Approved);
}

#[test]
fn governance_envelope_marks_external_guests_and_bridges_tainted() {
    let guest = RoomParticipant {
        id: PrincipalId::new("human:guest-1").unwrap(),
        display_name: "Guest".to_string(),
        handle: "guest".to_string(),
        kind: PrincipalKind::Human,
        role: PrincipalRole::ExternalGuest,
        scopes: vec![ScopeId::new("public").unwrap()],
        presence: PresenceStatus::Online,
    };

    let env = guest.governance_envelope(
        RoomId::new("cove-main").unwrap(),
        ChannelId::new("general").unwrap(),
        BridgeOrigin::CoveNative,
    );
    assert!(env.tainted_external);
    assert_eq!(env.actor_role, PrincipalRole::ExternalGuest);
}

#[test]
fn extract_mentions_parses_unique_normalized_handles() {
    let mentions = extract_mentions("Hey @Architect, please check with @security! cc @architect");
    assert_eq!(mentions, vec!["architect", "security"]);
}

#[test]
fn marlin_gateway_client_validates_base_url() {
    assert_eq!(
        MarlinGatewayClient::new("   ", None).unwrap_err(),
        MarlinGatewayError::InvalidBaseUrl
    );
    let client = MarlinGatewayClient::new("http://127.0.0.1:18093/", None).unwrap();
    assert_eq!(client.message_endpoint(), "http://127.0.0.1:18093/message");
}
