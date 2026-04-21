{{/*
Standard Helm helpers. Kept intentionally small — this chart avoids clever
templating so operators can diff rendered output against git history.
*/}}

{{- define "trackward.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "trackward.fullname" -}}
{{- if .Values.fullnameOverride -}}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- $name := default .Chart.Name .Values.nameOverride -}}
{{- if contains $name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}
{{- end -}}

{{- define "trackward.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" -}}
{{- end -}}

{{- define "trackward.labels" -}}
helm.sh/chart: {{ include "trackward.chart" . }}
app.kubernetes.io/name: {{ include "trackward.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end -}}

{{- define "trackward.ledger.selectorLabels" -}}
app.kubernetes.io/name: {{ include "trackward.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: ledger
{{- end -}}

{{- define "trackward.gateway.selectorLabels" -}}
app.kubernetes.io/name: {{ include "trackward.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: gateway
{{- end -}}

{{/*
Full container image reference. Centralized so `image.registry` and
`image.tag` overrides flow through one template.
*/}}
{{- define "trackward.image" -}}
{{- printf "%s/%s:%s" .Values.image.registry .Values.image.repository .Values.image.tag -}}
{{- end -}}

{{/*
Serialize gateway.toolRoutes (map) into the `name=url,name=url` format the
gateway's TOOL_ROUTES env parser expects.
*/}}
{{- define "trackward.gateway.toolRoutes" -}}
{{- $parts := list -}}
{{- range $k, $v := .Values.gateway.toolRoutes -}}
{{- $parts = append $parts (printf "%s=%s" $k $v) -}}
{{- end -}}
{{- join "," $parts -}}
{{- end -}}
