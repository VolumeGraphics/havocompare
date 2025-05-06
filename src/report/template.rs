pub const INDEX_FILENAME: &str = "index.html";
pub const DETAIL_FILENAME: &str = "detail.html";
pub const INDEX_TEMPLATE: &str = r##"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Report</title>
     <style>

        .error {
            background-color: #fbcccc !important;
        }

        h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		.container {
			padding:10px;
		}

		.dataTables_wrapper {
			font-family: monospace;
    		font-size: 10pt;
		}

		table.dataTable tbody td {
			padding:0px 0px !important;
		}
		
		.text-error {
			color:red;
		}

  		.ui-accordion-header-active:before {
		  	content: '-'
		}

		.ui-accordion-header-collapsed:before {
		  	content: '+'
		}

    </style>
    <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
</head>
<body>

<div id="accordion">
{% for rule_report in rule_results %}
	<h3>
		{{ rule_report.rule.name }}
	</h3>
	<div class="container">
	<table class="report cell-border">
		<thead>
		{% if rule_report.rule.FileProperties %}
			<tr>
				<th>File</th>
				<th colspan="2">File Size</th>
				<th colspan="2">Creation date</th>
				<th>Result</th>
			</tr>
			<tr>
				<th></th>
				<th>Nominal</th>
				<th>Actual</th>
				<th>Nominal</th>
				<th>Actual</th>
				<th></th>
			</tr>
		{% else %}
			<tr>
				<th>File</th>
				<th>Result</th>
			</tr>
		{% endif %}
		</thead>
		<tbody>
			{% for file in rule_report.diffs %}
				<tr {% if file.is_error %} class="error" {% endif %}>
					{% if rule_report.rule.FileProperties %}
						<td {% if file.additional_columns.0.is_error %} class="text-error" {% endif %}>
							{{ file.relative_file_path }}
						</td>
						<td {% if file.additional_columns.1.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.1.nominal_value }}
						</td>
						<td {% if file.additional_columns.1.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.1.actual_value }}
						</td>
						<td {% if file.additional_columns.2.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.2.nominal_value }}
						</td>
						<td {% if file.additional_columns.2.is_error %} class="text-error" {% endif %}>
							{{ file.additional_columns.2.actual_value }}
						</td>
						<td>{% if file.is_error %} <span class="text-error">&#10006;</span> {% else %} <span style="color:green;">&#10004;</span> {% endif %}</td>
					{% else %}
							<td>
								{% if file.detail_path %}
									<a href="./{{ rule_report.rule.name }}/{{ file.detail_path.name }}/{{ detail_filename }}">{{ file.relative_file_path }}</a>
								{% else %}
									{{ file.relative_file_path }}
								{% endif %}
							</td>
							<td>{% if file.is_error %} <span class="text-error">&#10006;</span> {% else %} <span style="color:green;">&#10004;</span> {% endif %}</td>
					{% endif %}
				</tr>
			{% endfor %}
		</tbody>
	</table>
	</div>
{% endfor %}
</div>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script
  src="https://code.jquery.com/ui/1.13.2/jquery-ui.min.js"
  integrity="sha256-lSjKY0/srUM9BE3dPm+c4fBo1dky2v27Gdjm2uoZaL0="
  crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('.report');
    });
    
    $(function() {
        $( "#accordion" ).accordion();
    });
</script>
</body>
</html>
"##;

pub const PLAIN_TEXT_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"#;

pub const PLAIN_IMAGE_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

        table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
            <tr>
                <td>{{ error }}</td>
            </tr>
    </tbody>
</table>

<p>
<h3>Nominal:</h3>
<img src="./{{ nominal_image }}" />
</p>

<p>
<h3>Actual:</h3>
<img src="./{{ actual_image }}" />
</p>

{% if diff_image %}
<p>
<h3>Diff:</h3>
<img src="./{{ diff_image }}" />
</p>
{% endif %}

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"#;

pub const PLAIN_CSV_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		.actual {
			color: #0d6efdf0;
		}
		
		.diffs {
			color: #FF4646;
		}
		
		table.dataTable {
			border: 1px solid #999999;
		}

		.dataTables_wrapper {
			font-family: monospace;
    		font-size: 10pt;
		}
		
		table.dataTable th:not(:last-child), table.dataTable td:not(:last-child) {
			border-right: 1px solid #999999;
		}

		.error {
            background-color: #fbcccc !important;
        }

        table.dataTable td, table.dataTable th {
			white-space:nowrap;
		}

		table.dataTable tbody td, table.dataTable thead th {
			padding:0px 0px;
		}
		
		.pre-text {
			white-space:pre;
		}

    </style>
</head>
<body>

<h3>Compare Result</h3>
<p>
	<table>
		<tbody>
			<tr>
				<td>Left file (nominal):</td>
				<td>{{ nominal }}</td>
			</tr>
				<tr>
				<td>Right file (actual):</td>
				<td>{{ actual }}</td>
			</tr>
		</tbody>
	</table>
</p>

{% if headers.columns|length <= 0 %}
	<p><i>Header preprocessing not enabled in config</i></p>
{% endif %}
<table id="report" class="cell-border">
    <thead>
    {% if headers.columns|length > 0 %}
    	<tr {% if headers.has_diff %} class="error" {% endif %}>
	    	<th>{% if headers.has_diff %}x<br>&nbsp;{% endif %}</th>
	    	<th>{% if headers.has_diff %}&nbsp;<br>x{% endif %}</th>
			{% for col in headers.columns %}
				<th>
					{{ col.nominal_value }}
					{% if headers.has_diff %}
						<div class="{% if col.nominal_value != col.actual_value %} diffs {% endif %}">{{ col.actual_value }}</div>
					{% endif %}
				</th>
			{% endfor %}
		</tr>
    {% else %}
	    <tr>
	    	<th>&nbsp;</th>
	    	<th>&nbsp;</th>
	    	{% for col in rows[0].columns %}
		    	<th>&nbsp;</th>
	    	{% endfor %}
	    </tr>
    {% endif %}
    </thead>
    <tbody>
        {% for row in rows %}
            <tr {% if row.has_error %} class="error" {% endif %}>
            	<td data-order="{{ loop.index0 }}">{{ loop.index0 }}{% if row.has_diff or row.has_error %}<br>&nbsp;{% endif %}</td>
            	<td data-order="{{ loop.index0 }}">{% if row.has_diff or row.has_error %}&nbsp;<br>{% endif %}{{ loop.index0 }}</td>
            	{% for col in row.columns %}
					<td>
					<span class="pre-text">{{ col.nominal_value }}</span>
					{% if row.has_diff or row.has_error %}
						<div class="{% if col.diffs|length > 0 %} diffs {% elif col.nominal_value != col.actual_value %} actual {% else %} {% endif %}">
						<span class="pre-text">{{ col.actual_value }}</span>
						</div>
					{% endif %}
                	 </td>
                {% endfor %}
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report', {
			lengthMenu: [ [5, 10, 25, 50, -1], [5, 10, 25, 50, "All"] ],
			iDisplayLength: -1,
			columnDefs: [
    			{ type: "num", "targets": 0 },
    			{ type: "num", "targets": 1 }
			],
			bPaginate: false,
    		bLengthChange: false,
		});
    });
</script>

</body>
</html>
"#;

pub const PLAIN_PDF_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

        .helper {
        	color:orange;
        	font-weight:bold;
        }

		.helper a {
			color:orange;
		}

		.has_diff {
			color: #0d6efdf0;
		}

		.has_error {
			color:red;
		}

		#compare th {
			text-align:left;
			background-color: #cccccc;
			padding:10px;
		}

		#compare td:first-child {
			border-right: 1px solid black;
		}

		table#compare {
			border:1px solid grey;
		}
		
		.pre-text {
			white-space:pre;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<div class="helper">
This is for viewing only.
The extracted exact text can be downloaded here: <a href="./{{ nominal_extracted_filename }}">nominal</a> and <a href="./{{ actual_extracted_filename }}">actual</a>
</div>
<table id="compare">
	<thead>
		<tr>
			<th></th>
			<th>Nominal</th>
			<th>Actual</th>
		</tr>
	</thead>
	<tbody>
	{% for line in combined_lines %}
		<tr>
			<td>{{ loop.index }}</td>
			<td><span class="pre-text">{{ line.nominal_value|safe }}</span></td>
			<td>
				{% if line.diffs|length > 0 %}
					<span class="pre-text has_error">{{ line.actual_value|safe }}</span>
				{% elif line.actual_value != line.nominal_value %}
					<span class="pre-text has_diff">{{ line.actual_value|safe }}</span>
				{% else %}
					<span class="pre-text">{{ line.actual_value|safe }}</span>
				{% endif %}
			</td>
		</tr>
	{% endfor %}
	</tbody>
</table>

<br>
<br>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report');
    });
</script>

</body>
</html>
"#;

pub const ERROR_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
     <title>Error(s)</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>
     
     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}
		
		table {
		  table-layout: fixed;
		}

        table.dataTable#report tbody tr {
            background-color: #fbcccc;
        }

    </style>
</head>
<body>

<p>
	<table>
		<tbody>
			<tr>
				<td>Nominal:</td>
				<td>{{ nominal }}</td>
			</tr>
				<tr>
				<td>Actual:</td>
				<td>{{ actual }}</td>
			</tr>
		</tbody>
	</table>
</p>

<table id="report">
    <thead>
    <tr>
        <th>Error</th>
    </tr>
    </thead>
    <tbody>
        {% for error in errors %}
            <tr>
                <td>{{ error }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report', {
        	iDisplayLength: -1,
			bPaginate: false,
    		bLengthChange: false,
    		bFilter: false,
    		bInfo: false
        });
    });
</script>

</body>
</html>
"#;

pub const PLAIN_EXTERNAL_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

		.has_diff {
			color: #0d6efdf0;
		}

		.has_error {
			color:red;
		}

		#compare th {
			text-align:left;
			background-color: #cccccc;
			padding:10px;
		}

		#compare td {
			white-space: pre;
		}

		#compare td:first-child {
			border-right: 1px solid black;
		}

		table#compare {
			border:1px solid grey;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>

<table id="compare">
	<thead>
		<tr>
			<th>Stdout</th>
			<th>Stderr</th>
		</tr>
	</thead>
	<tbody>
		<tr>
			<td>
{{ stdout }}
			</td>
			<td class="has_error">
{{ stderr }}
			</td>
		</tr>
	</tbody>
</table>


</body>
</html>
"#;

pub const PLAIN_JSON_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		table {
		  table-layout: fixed;
		}

        table.dataTable tr.odd {
            background-color: #dddddd;
        }

		.has_diff {
			color: #0d6efdf0;
		}

		.has_right {
			color:green;
		}
		.has_left {
			color:red;
		}

		#compare th {
			text-align:left;
			background-color: #cccccc;
			padding:10px;
		}

		#compare td {
			white-space: pre;
		}

		#compare td:first-child {
			border-right: 1px solid black;
		}

		table#compare {
			border:1px solid grey;
		}

    </style>
</head>
<body>

<h3>Compare Result of {{ actual }} and {{ nominal }}</h3>
<div>{{ root_mismatch }} </div>
<table id="compare">
	<thead>
		<tr>
			<th>Left extra</th>
			<th>Differences</th>
			<th>Right extra</th>
		</tr>
	</thead>
	<tbody>
		<tr>
			<td class="has_left">
{{ left }}
			</td>
			<td >
{{ differences }}
			</td>
			<td class="has_right">
{{ right }}
			</td>
		</tr>
	</tbody>
</table>


</body>
</html>
"#;

pub const FILE_EXIST_DETAIL_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <title>Results</title>
     <link rel="stylesheet" type="text/css" href="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.css"/>

     <style>

   		h3 {
			background-color:black;
			color:white;
			padding:10px;
			margin:10px 0;
			cursor:pointer;
		}

		.actual {
			color: #0d6efdf0;
		}

		.diffs {
			color: #FF4646;
		}

		table.dataTable {
			border: 1px solid #999999;
		}

		.dataTables_wrapper {
			font-family: monospace;
    		font-size: 10pt;
		}

		table.dataTable th:not(:last-child), table.dataTable td:not(:last-child) {
			border-right: 1px solid #999999;
		}

		.error {
            background-color: #fbcccc !important;
        }

        table.dataTable td, table.dataTable th {
			white-space:nowrap;
		}

		table.dataTable tbody td, table.dataTable thead th {
			padding:0px 0px;
		}

		.pre-text {
			white-space:pre;
		}

    </style>
</head>
<body>

<h3>Compare Result</h3>

<p>Mode: {{ mode }}</p>

<table id="report" class="cell-border">
    <thead>
	    <tr>
	    	<th>nominal: {{ nominal }}</th>
	    	<th>actual: {{ actual }}</th>
	    </tr>
    </thead>
    <tbody>
        {% for row in rows %}
            <tr {% if row.2 %} class="error" {% endif %}>
            	<td>{{ row.0 }}</td>
            	<td>{{ row.1 }}</td>
            </tr>
        {% endfor %}
    </tbody>
</table>

<script src="https://code.jquery.com/jquery-3.6.0.min.js" integrity="sha256-/xUj+3OJU5yExlq6GSYGSHk7tPXikynS7ogEvDej/m4=" crossorigin="anonymous"></script>
<script type="text/javascript" src="https://cdn.datatables.net/v/dt/dt-1.12.1/datatables.min.js"></script>
<script>
    document.addEventListener('DOMContentLoaded', function () {
        let table = new DataTable('#report', {
			lengthMenu: [ [5, 10, 25, 50, -1], [5, 10, 25, 50, "All"] ],
			iDisplayLength: -1,
			columnDefs: [
    			{ type: "num", "targets": 0 },
    			{ type: "num", "targets": 1 }
			],
			bPaginate: false,
    		bLengthChange: false,
		});
    });
</script>

</body>
</html>
"#;
