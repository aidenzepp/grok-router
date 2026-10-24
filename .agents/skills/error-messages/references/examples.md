# Error Message Examples

These examples combine guidance from Wix UX, Google Technical Writing, Google AIP-193, and Material Design.

## UI Modal

Weak:

```text
Whoops! Something went wrong.
The third-party you are trying to connect to is not responding, so we can't fetch your data. Try again later.
```

Problems:

- cute tone
- passes blame to another provider
- uses implementation wording like "fetch"
- gives no confidence about saved state
- offers no fallback if retry fails

Better:

```text
Unable to connect your account
Your changes were saved, but we could not connect the account because the provider is unavailable. Try again later, or contact support if this keeps happening.
```

Why it works:

- says what happened
- explains why at the right level
- reassures the user
- offers a fix and fallback

## Inline Form Validation

Weak:

```text
Invalid postal code.
```

Better:

```text
Enter a 5-digit ZIP code
```

Better when the surface can show more detail:

```text
The ZIP code must be 5 digits. You entered 4872953.
```

Do not show the error before the user interacts with the field or submits the form. Preserve the user's input so they can fix it.

## Permissions

Weak:

```text
Permission denied.
```

Better for an end-user UI:

```text
You need billing access to download invoices. Ask an account owner to update your role.
```

Better for an API:

```json
{
  "error": {
    "code": 403,
    "status": "PERMISSION_DENIED",
    "message": "You need billing.viewer access to download invoice inv_123.",
    "details": [
      {
        "@type": "type.googleapis.com/google.rpc.ErrorInfo",
        "reason": "MISSING_REQUIRED_ROLE",
        "domain": "billing.example.com",
        "metadata": {
          "resource": "invoices/inv_123",
          "requiredRole": "billing.viewer"
        }
      }
    ]
  }
}
```

## Quota Or Limit

Weak:

```text
Quota exceeded.
```

Better:

```text
You requested 2 CPUs, which exceeds your quota of 1 CPU in us-central-1. Request a quota increase or choose a region with available quota.
```

## CLI Error

Weak:

```text
Could not read config.
```

Better:

```text
Cannot read config file at /app/config.yml: permission denied. Give the current user read access or pass --config with a readable file.
```

Code shape:

```ts
throw new CliError({
  code: "CONFIG_NOT_READABLE",
  message:
    `Cannot read config file at ${configPath}: ${reason}. ` +
    "Give the current user read access or pass --config with a readable file.",
  context: {
    configPath,
    reason,
  },
});
```

## API Validation Code Pattern

Prefer distinct, structured violations over one generic validation message.

Weak:

```ts
return res.status(400).json({
  error: "Invalid request",
});
```

Better:

```ts
return res.status(400).json({
  error: {
    code: "INVALID_ARGUMENT",
    message: "The request has 2 invalid fields.",
    details: [
      {
        reason: "FIELD_VIOLATION",
        fieldViolations: [
          {
            field: "email",
            description: "Enter an email address that includes @ and a domain.",
          },
          {
            field: "seatCount",
            description: "Seat count must be between 1 and 50. You entered 72.",
            context: {
              min: 1,
              max: 50,
              actual: 72,
            },
          },
        ],
      },
    ],
  },
});
```

## Separate Project Example: Billing CSV Import

Scenario: a SaaS billing app lets account admins upload a CSV of customer invoices.

Before:

```text
Upload failed. Invalid file.
```

```text
Bad date.
```

```json
{
  "error": "CSV validation failed"
}
```

Problems:

- generic
- no row or field location
- no preserved-state reassurance
- no constraint
- no stable machine-readable detail for branching or automation

Better UI summary:

```text
We couldn't import 12 invoices because some rows need fixes. Your file is saved as a draft.
```

Better inline row error near `due_date`:

```text
Due date must be after the invoice date. Change row 8 to a date after May 14, 2026.
```

Recovery actions:

```text
Fix rows in draft
Download error report
Cancel import
```

Better API response:

```json
{
  "code": "invoice_import_validation_failed",
  "message": "Some invoice rows need fixes before import.",
  "details": [
    {
      "code": "due_date_before_invoice_date",
      "row": 8,
      "field": "due_date",
      "message": "Due date must be after the invoice date.",
      "context": {
        "minExclusive": "2026-05-14"
      }
    }
  ],
  "context": {
    "draftImportId": "imp_7x92",
    "uploadedFileName": "may-invoices.csv",
    "rejectedRows": 12,
    "availableActions": ["edit_draft", "download_report"]
  }
}
```

Code representation:

```ts
type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type ErrorPayload<TDetail = JsonValue> = {
  code: string;
  message: string;
  details: TDetail[];
  context?: Record<string, JsonValue>;
};

type ImportErrorDetail = {
  code: string;
  row: number;
  field: string;
  message: string;
  context?: {
    minExclusive?: string;
  };
};

type ImportErrorResponse = ErrorPayload<ImportErrorDetail>;
```

```ts
function dueDateBeforeInvoiceDateError(
  row: number,
  invoiceDate: string,
): ImportErrorDetail {
  return {
    code: "due_date_before_invoice_date",
    row,
    field: "due_date",
    message: "Due date must be after the invoice date.",
    context: {
      minExclusive: invoiceDate,
    },
  };
}
```

```tsx
function ImportErrorSummary({ error }: { error: ImportErrorResponse }) {
  return (
    <section role="alert" aria-labelledby="import-error-title">
      <h2 id="import-error-title">{error.message}</h2>
      <p>
        Your file is saved as a draft: {String(error.context?.uploadedFileName)}.
      </p>

      <ul>
        {error.details.map((detail) => (
          <li key={`${detail.row}-${detail.field}-${detail.code}`}>
            Row {detail.row}, {detail.field}: {detail.message}
          </li>
        ))}
      </ul>
    </section>
  );
}
```

Why it works:

- says what happened: some invoice rows could not be imported
- says why: the due date violates a specific constraint
- says what was preserved: the upload is saved as a draft
- says how to fix it: edit row 8 and choose a due date after the invoice date
- gives a way out: download the report or cancel the import
- keeps prose human-facing while giving machines stable `code`, `field`, `row`, and contextual constraint values

## Boundary Payload Pattern

Use a small payload at presentation boundaries rather than making every internal error type carry every possible field:

```ts
type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type ErrorPayload<TDetail = JsonValue> = {
  code: string;
  message: string;
  details: TDetail[];
  context?: Record<string, JsonValue>;
};
```

Example:

```ts
type FieldViolation = {
  field: string;
  reason: string;
  message: string;
  context?: Record<string, JsonValue>;
};

const error: ErrorPayload<FieldViolation> = {
  code: "invoice_import_validation_failed",
  message:
    "We couldn't import 12 invoices because some rows need fixes. Your file is saved as a draft.",
  details: [
    {
      field: "due_date",
      reason: "before_invoice_date",
      message: "Due date must be after the invoice date.",
      context: {
        row: 8,
        minExclusive: "2026-05-14",
      },
    },
  ],
  context: {
    draftImportId: "imp_7x92",
    uploadedFileName: "may-invoices.csv",
    rejectedRows: 12,
    availableActions: ["edit_draft", "download_report"],
  },
};
```

Use `details` for repeated or item-specific repair facts. Use `context` for facts that apply to the whole error. Do not add optional top-level fields unless the API already standardizes them.

## Go Boundary Example

In Go, keep service errors idiomatic and render the boundary payload later.

```go
var ErrWorkspaceNameTaken = errors.New("workspace name taken")

type FieldViolation struct {
	Field   string `json:"field"`
	Reason  string `json:"reason"`
	Message string `json:"message"`
}

type ValidationError struct {
	Violations []FieldViolation
}

func (e ValidationError) Error() string {
	return "validation failed"
}

func CreateWorkspace(ctx context.Context, name string) error {
	if strings.TrimSpace(name) == "" {
		return ValidationError{Violations: []FieldViolation{{
			Field:   "name",
			Reason:  "required",
			Message: "Enter a workspace name.",
		}}}
	}

	if exists(name) {
		return fmt.Errorf("%w: %q", ErrWorkspaceNameTaken, name)
	}

	return nil
}
```

Render at the HTTP/API boundary:

```go
type ErrorPayload[T any] struct {
	Code    string         `json:"code"`
	Message string         `json:"message"`
	Details []T            `json:"details"`
	Context map[string]any `json:"context,omitempty"`
}

func WorkspaceProblemFromError(err error) (int, ErrorPayload[FieldViolation]) {
	var verr ValidationError
	if errors.As(err, &verr) {
		return http.StatusBadRequest, ErrorPayload[FieldViolation]{
			Code:    "workspace_validation_failed",
			Message: "Workspace details need fixes.",
			Details: verr.Violations,
		}
	}

	if errors.Is(err, ErrWorkspaceNameTaken) {
		return http.StatusConflict, ErrorPayload[FieldViolation]{
			Code:    "workspace_name_taken",
			Message: "Workspace name is already in use. Choose a different name.",
			Details: []FieldViolation{},
		}
	}

	return http.StatusInternalServerError, ErrorPayload[FieldViolation]{
		Code:    "unexpected_error",
		Message: "Unable to create workspace. Try again later.",
		Details: []FieldViolation{},
	}
}
```

## Quick Rewrite Table

| Weak | Better |
| --- | --- |
| Something went wrong | We could not save the report. Your edits are still open. Try saving again. |
| Invalid input | Enter a date in MM/DD/YYYY format |
| Failed | Upload failed because the file is larger than 10 MB |
| Network error | You are offline. Reconnect to keep editing shared documents. |
| Access denied | Only workspace admins can invite new members |
| Try again later | The payment provider is unavailable. Try again in a few minutes or use a different payment method. |
