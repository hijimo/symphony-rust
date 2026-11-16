---
symphony_template:
  prompt: |
    Read the attached intent.md and produce a requirements and design spec for integrating it into the existing codebase. Apply the organization skills (security-policy) and AGENTS.md. Under ## Concerns, list every security or data-protection question with an owner and a resolution. Document the spec fully as spec.md.
  checklist:
    - spec 是否解决了 intent 中陈述的问题？
    - 对外接口是否写明鉴权方式和审计日志？
    - 涉及个人数据时是否写明数据类别、保存位置和访问控制？
    - 标出的关注点是否都有负责人和结论？
---
# Spec: <title>

## Requirements
<!-- 需求与验收标准 -->

## Design
<!-- 如何集成进现有代码库：接口（含鉴权）、数据、交互 -->

## Concerns
<!-- 每条一行，写明 owner: <负责人> resolution: <结论>；没有则写“无” -->
