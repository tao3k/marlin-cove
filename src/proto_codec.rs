// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed conversions between Marlin Cove domain models and `marlin.cove.v1` protobufs.

use thiserror::Error;

use crate::proto_wire::cove_v1;
use crate::protocol::{
    BridgeOrigin, ChannelId, GateApprovalReceipt, GateDecision, GateId, MessageAttachment,
    MessageId, PresenceStatus, PrincipalId, PrincipalKind, PrincipalRole, ProtocolError,
    RoomChannel, RoomEvent, RoomEventKind, RoomGate, RoomGovernanceEnvelope, RoomId, RoomInvite,
    RoomMessage, RoomParticipant, RoomSnapshot, ScopeId,
};

/// Errors returned when decoding invalid or unspecified protobuf fields into domain types.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtoCodecError {
    /// Required nested protobuf message field was missing.
    #[error("missing required protobuf field `{field}`")]
    MissingField {
        /// Missing field name.
        field: &'static str,
    },
    /// Enum wire tag was unspecified or unrecognized.
    #[error("invalid enum value `{value}` for `{field}`")]
    InvalidEnum {
        /// Protobuf enum field name.
        field: &'static str,
        /// Unrecognized wire discriminant.
        value: i32,
    },
    /// Domain identifier validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

/// Convert domain [`PrincipalKind`] into its `marlin.cove.v1` wire discriminant.
#[must_use]
pub const fn principal_kind_to_proto(kind: PrincipalKind) -> i32 {
    match kind {
        PrincipalKind::Human => cove_v1::PrincipalKind::Human as i32,
        PrincipalKind::Agent => cove_v1::PrincipalKind::Agent as i32,
    }
}

/// Decode a `marlin.cove.v1` [`cove_v1::PrincipalKind`] discriminant into [`PrincipalKind`].
///
/// # Errors
///
/// Returns [`ProtoCodecError::InvalidEnum`] when the value is unspecified or unknown.
pub fn principal_kind_from_proto(value: i32) -> Result<PrincipalKind, ProtoCodecError> {
    match cove_v1::PrincipalKind::try_from(value) {
        Ok(cove_v1::PrincipalKind::Human) => Ok(PrincipalKind::Human),
        Ok(cove_v1::PrincipalKind::Agent) => Ok(PrincipalKind::Agent),
        _ => Err(ProtoCodecError::InvalidEnum {
            field: "principal_kind",
            value,
        }),
    }
}

/// Convert domain [`PrincipalRole`] into its `marlin.cove.v1` wire discriminant.
#[must_use]
pub const fn principal_role_to_proto(role: PrincipalRole) -> i32 {
    match role {
        PrincipalRole::Owner => cove_v1::PrincipalRole::Owner as i32,
        PrincipalRole::Maintainer => cove_v1::PrincipalRole::Maintainer as i32,
        PrincipalRole::Member => cove_v1::PrincipalRole::Member as i32,
        PrincipalRole::ExternalGuest => cove_v1::PrincipalRole::ExternalGuest as i32,
        PrincipalRole::RoleAgent => cove_v1::PrincipalRole::RoleAgent as i32,
    }
}

/// Decode a `marlin.cove.v1` [`cove_v1::PrincipalRole`] discriminant into [`PrincipalRole`].
///
/// # Errors
///
/// Returns [`ProtoCodecError::InvalidEnum`] when the value is unspecified or unknown.
pub fn principal_role_from_proto(value: i32) -> Result<PrincipalRole, ProtoCodecError> {
    match cove_v1::PrincipalRole::try_from(value) {
        Ok(cove_v1::PrincipalRole::Owner) => Ok(PrincipalRole::Owner),
        Ok(cove_v1::PrincipalRole::Maintainer) => Ok(PrincipalRole::Maintainer),
        Ok(cove_v1::PrincipalRole::Member) => Ok(PrincipalRole::Member),
        Ok(cove_v1::PrincipalRole::ExternalGuest) => Ok(PrincipalRole::ExternalGuest),
        Ok(cove_v1::PrincipalRole::RoleAgent) => Ok(PrincipalRole::RoleAgent),
        _ => Err(ProtoCodecError::InvalidEnum {
            field: "principal_role",
            value,
        }),
    }
}

/// Convert domain [`PresenceStatus`] into its `marlin.cove.v1` wire discriminant.
#[must_use]
pub const fn presence_to_proto(presence: PresenceStatus) -> i32 {
    match presence {
        PresenceStatus::Online => cove_v1::PresenceStatus::Online as i32,
        PresenceStatus::Thinking => cove_v1::PresenceStatus::Thinking as i32,
        PresenceStatus::Working => cove_v1::PresenceStatus::Working as i32,
        PresenceStatus::Idle => cove_v1::PresenceStatus::Idle as i32,
        PresenceStatus::Offline => cove_v1::PresenceStatus::Offline as i32,
    }
}

/// Convert domain [`BridgeOrigin`] into its `marlin.cove.v1` wire discriminant.
#[must_use]
pub const fn bridge_origin_to_proto(origin: BridgeOrigin) -> i32 {
    match origin {
        BridgeOrigin::CoveNative => cove_v1::BridgeOrigin::CoveNative as i32,
        BridgeOrigin::Telegram => cove_v1::BridgeOrigin::Telegram as i32,
        BridgeOrigin::Discord => cove_v1::BridgeOrigin::Discord as i32,
        BridgeOrigin::MarlinGateway => cove_v1::BridgeOrigin::MarlinGateway as i32,
    }
}

/// Decode a `marlin.cove.v1` [`cove_v1::BridgeOrigin`] discriminant, defaulting unspecified to native.
///
/// # Errors
///
/// Returns [`ProtoCodecError::InvalidEnum`] when the wire tag is unrecognized.
pub fn bridge_origin_from_proto(value: i32) -> Result<BridgeOrigin, ProtoCodecError> {
    match cove_v1::BridgeOrigin::try_from(value) {
        Ok(cove_v1::BridgeOrigin::Unspecified | cove_v1::BridgeOrigin::CoveNative) => {
            Ok(BridgeOrigin::CoveNative)
        }
        Ok(cove_v1::BridgeOrigin::Telegram) => Ok(BridgeOrigin::Telegram),
        Ok(cove_v1::BridgeOrigin::Discord) => Ok(BridgeOrigin::Discord),
        Ok(cove_v1::BridgeOrigin::MarlinGateway) => Ok(BridgeOrigin::MarlinGateway),
        Err(_) => Err(ProtoCodecError::InvalidEnum {
            field: "origin",
            value,
        }),
    }
}

/// Convert domain [`GateDecision`] into its `marlin.cove.v1` wire discriminant.
#[must_use]
pub const fn gate_decision_to_proto(decision: GateDecision) -> i32 {
    match decision {
        GateDecision::Pending => cove_v1::GateDecision::Pending as i32,
        GateDecision::Approved => cove_v1::GateDecision::Approved as i32,
        GateDecision::Denied => cove_v1::GateDecision::Denied as i32,
    }
}

/// Encode [`MessageAttachment`] to protobuf.
#[must_use]
pub fn attachment_to_proto(attachment: &MessageAttachment) -> cove_v1::MessageAttachment {
    match attachment {
        MessageAttachment::ImageUrl { url } => cove_v1::MessageAttachment {
            kind: Some(cove_v1::message_attachment::Kind::ImageUrl(url.clone())),
        },
    }
}

/// Decode [`cove_v1::MessageAttachment`] to domain [`MessageAttachment`].
///
/// # Errors
///
/// Returns [`ProtoCodecError::MissingField`] if the oneof variant is absent.
pub fn attachment_from_proto(
    proto: cove_v1::MessageAttachment,
) -> Result<MessageAttachment, ProtoCodecError> {
    match proto.kind {
        Some(cove_v1::message_attachment::Kind::ImageUrl(url)) => {
            Ok(MessageAttachment::ImageUrl { url })
        }
        None => Err(ProtoCodecError::MissingField {
            field: "attachment.kind",
        }),
    }
}

/// Encode [`RoomGovernanceEnvelope`] to protobuf.
#[must_use]
pub fn governance_envelope_to_proto(
    envelope: &RoomGovernanceEnvelope,
) -> cove_v1::RoomGovernanceEnvelope {
    cove_v1::RoomGovernanceEnvelope {
        room_id: envelope.room_id.to_string(),
        channel_id: envelope.channel_id.to_string(),
        actor_id: envelope.actor_id.to_string(),
        actor_kind: principal_kind_to_proto(envelope.actor_kind),
        actor_role: principal_role_to_proto(envelope.actor_role),
        granted_scopes: envelope
            .granted_scopes
            .iter()
            .map(ToString::to_string)
            .collect(),
        origin: bridge_origin_to_proto(envelope.origin),
        tainted_external: envelope.tainted_external,
    }
}

/// Decode [`cove_v1::RoomGovernanceEnvelope`] to domain [`RoomGovernanceEnvelope`].
///
/// # Errors
///
/// Returns [`ProtoCodecError`] when identifiers or enum tags are invalid.
pub fn governance_envelope_from_proto(
    proto: cove_v1::RoomGovernanceEnvelope,
) -> Result<RoomGovernanceEnvelope, ProtoCodecError> {
    let granted_scopes = proto
        .granted_scopes
        .into_iter()
        .map(ScopeId::new)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RoomGovernanceEnvelope {
        room_id: RoomId::new(proto.room_id)?,
        channel_id: ChannelId::new(proto.channel_id)?,
        actor_id: PrincipalId::new(proto.actor_id)?,
        actor_kind: principal_kind_from_proto(proto.actor_kind)?,
        actor_role: principal_role_from_proto(proto.actor_role)?,
        granted_scopes,
        origin: bridge_origin_from_proto(proto.origin)?,
        tainted_external: proto.tainted_external,
    })
}

/// Encode [`RoomParticipant`] to protobuf.
#[must_use]
pub fn participant_to_proto(participant: &RoomParticipant) -> cove_v1::RoomParticipant {
    cove_v1::RoomParticipant {
        id: participant.id.to_string(),
        display_name: participant.display_name.clone(),
        handle: participant.handle.clone(),
        kind: principal_kind_to_proto(participant.kind),
        role: principal_role_to_proto(participant.role),
        scopes: participant.scopes.iter().map(ToString::to_string).collect(),
        presence: presence_to_proto(participant.presence),
    }
}

/// Encode [`RoomChannel`] to protobuf.
#[must_use]
pub fn channel_to_proto(channel: &RoomChannel) -> cove_v1::RoomChannel {
    cove_v1::RoomChannel {
        id: channel.id.to_string(),
        name: channel.name.clone(),
        topic: channel.topic.clone(),
        is_direct: channel.is_direct,
    }
}

/// Encode [`RoomInvite`] to protobuf.
#[must_use]
pub fn invite_to_proto(invite: &RoomInvite) -> cove_v1::RoomInvite {
    cove_v1::RoomInvite {
        token: invite.token.to_string(),
        room_id: invite.room_id.to_string(),
        issued_by: invite.issued_by.to_string(),
        grant_role: principal_role_to_proto(invite.grant_role),
        grant_scopes: invite
            .grant_scopes
            .iter()
            .map(ToString::to_string)
            .collect(),
        max_uses: invite.max_uses,
        uses: invite.uses,
    }
}

/// Encode [`RoomMessage`] to protobuf.
#[must_use]
pub fn message_to_proto(message: &RoomMessage) -> cove_v1::RoomMessage {
    cove_v1::RoomMessage {
        id: message.id.to_string(),
        room_id: message.room_id.to_string(),
        channel_id: message.channel_id.to_string(),
        thread_root_id: message
            .thread_root_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        author_id: message.author_id.to_string(),
        author_kind: principal_kind_to_proto(message.author_kind),
        body: message.body.clone(),
        mentions: message.mentions.clone(),
        gate_id: message
            .gate_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        attachments: message
            .attachments
            .iter()
            .map(attachment_to_proto)
            .collect(),
        origin: bridge_origin_to_proto(message.origin),
        bot_chain_depth: u32::from(message.bot_chain_depth),
        created_at_unix: message.created_at_unix,
    }
}

/// Encode [`GateApprovalReceipt`] to protobuf.
#[must_use]
pub fn receipt_to_proto(receipt: &GateApprovalReceipt) -> cove_v1::GateApprovalReceipt {
    cove_v1::GateApprovalReceipt {
        gate_id: receipt.gate_id.to_string(),
        resolved_by: receipt.resolved_by.to_string(),
        decision: gate_decision_to_proto(receipt.decision),
        reason: receipt.reason.clone(),
        resolved_at_unix: receipt.resolved_at_unix,
    }
}

/// Encode [`RoomGate`] to protobuf.
#[must_use]
pub fn gate_to_proto(gate: &RoomGate) -> cove_v1::RoomGate {
    cove_v1::RoomGate {
        id: gate.id.to_string(),
        room_id: gate.room_id.to_string(),
        channel_id: gate.channel_id.to_string(),
        requested_by: gate.requested_by.to_string(),
        summary: gate.summary.clone(),
        capability: gate.capability.clone(),
        decision: gate_decision_to_proto(gate.decision),
        receipt: gate.receipt.as_ref().map(receipt_to_proto),
    }
}

/// Encode [`RoomSnapshot`] to protobuf.
#[must_use]
pub fn snapshot_to_proto(snapshot: &RoomSnapshot) -> cove_v1::RoomSnapshot {
    cove_v1::RoomSnapshot {
        room_id: snapshot.room_id.to_string(),
        title: snapshot.title.clone(),
        latest_seq: snapshot.latest_seq,
        channels: snapshot.channels.iter().map(channel_to_proto).collect(),
        participants: snapshot
            .participants
            .iter()
            .map(participant_to_proto)
            .collect(),
        messages: snapshot.messages.iter().map(message_to_proto).collect(),
        gates: snapshot.gates.iter().map(gate_to_proto).collect(),
    }
}

/// Encode [`RoomEvent`] to protobuf.
#[must_use]
pub fn event_to_proto(event: &RoomEvent) -> cove_v1::RoomEvent {
    let kind = match &event.kind {
        RoomEventKind::MessagePosted { message } => {
            cove_v1::room_event::Kind::MessagePosted(message_to_proto(message))
        }
        RoomEventKind::ParticipantJoined { participant } => {
            cove_v1::room_event::Kind::ParticipantJoined(participant_to_proto(participant))
        }
        RoomEventKind::PresenceChanged {
            principal_id,
            presence,
        } => cove_v1::room_event::Kind::PresenceChanged(cove_v1::PresenceChangedEvent {
            principal_id: principal_id.to_string(),
            presence: presence_to_proto(*presence),
        }),
        RoomEventKind::GateRequested { gate } => {
            cove_v1::room_event::Kind::GateRequested(gate_to_proto(gate))
        }
        RoomEventKind::GateResolved { receipt } => {
            cove_v1::room_event::Kind::GateResolved(receipt_to_proto(receipt))
        }
    };
    cove_v1::RoomEvent {
        seq: event.seq,
        room_id: event.room_id.to_string(),
        kind: Some(kind),
    }
}

/// Parse an optional non-empty [`MessageId`] from a protobuf string.
///
/// # Errors
///
/// Returns [`ProtocolError`] if non-empty parsing fails.
pub fn optional_message_id(raw: &str) -> Result<Option<MessageId>, ProtocolError> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        MessageId::new(raw).map(Some)
    }
}

/// Parse an optional non-empty [`GateId`] from a protobuf string.
///
/// # Errors
///
/// Returns [`ProtocolError`] if non-empty parsing fails.
pub fn optional_gate_id(raw: &str) -> Result<Option<GateId>, ProtocolError> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        GateId::new(raw).map(Some)
    }
}
